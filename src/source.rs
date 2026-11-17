//! The Google Analytics data source: report queries turned into `runReport`
//! requests, and Google's rows turned back into report rows.

use std::time::Duration;

use chrono::{Months, NaiveDate};
use laterite_core::serde_json::{json, Value};
use laterite_core::{t, SealedFile, Text};
use rainmill_reporting::{
    Aggregate, DataSource, Dimension, FilterOp, Group, Metric, OrderBy, ReportCx, ReportData,
    ReportError, ReportQuery, Row, Unit,
};

use crate::google::{Analytics, ServiceAccount};
use crate::{SLOT, VIEW};

/// What Google reports for a value it could not set.
const NOT_SET: &str = "(not set)";

/// Our dimension code, its label and Google's field.
fn dimension_table() -> Vec<(&'static str, Text, &'static str)> {
    vec![
        ("page_path", t!("Page path"), "pagePath"),
        ("page_title", t!("Page title"), "pageTitle"),
        ("landing_page", t!("Landing page"), "landingPage"),
        ("hostname", t!("Hostname"), "hostName"),
        ("source", t!("Source"), "sessionSource"),
        ("medium", t!("Medium"), "sessionMedium"),
        (
            "source_medium",
            t!("Source / medium"),
            "sessionSourceMedium",
        ),
        ("channel", t!("Channel"), "sessionDefaultChannelGroup"),
        ("campaign", t!("Campaign"), "sessionCampaignName"),
        ("referrer", t!("Referrer"), "pageReferrer"),
        ("first_source", t!("First user source"), "firstUserSource"),
        (
            "first_channel",
            t!("First user channel"),
            "firstUserDefaultChannelGroup",
        ),
        ("country", t!("Country"), "country"),
        ("region", t!("Region"), "region"),
        ("city", t!("City"), "city"),
        ("language", t!("Language"), "language"),
        ("device", t!("Device"), "deviceCategory"),
        ("os", t!("Operating system"), "operatingSystem"),
        ("browser", t!("Browser"), "browser"),
        ("screen", t!("Screen resolution"), "screenResolution"),
        ("new_or_returning", t!("New or returning"), "newVsReturning"),
        ("event", t!("Event"), "eventName"),
        ("key_event", t!("Key event"), "isKeyEvent"),
        ("weekday", t!("Day of week"), "dayOfWeekName"),
        ("hour", t!("Hour"), "hour"),
        ("outbound", t!("Outbound link"), "linkDomain"),
        ("file", t!("File"), "fileName"),
        ("search_term", t!("Site search term"), "searchTerm"),
    ]
}

/// Our metric code, its label, Google's field, how it combines and reads.
fn metric_table() -> Vec<(&'static str, Text, &'static str, Aggregate, Unit)> {
    use Aggregate::{Average, CountDistinct, Sum};
    vec![
        ("views", t!("Views"), "screenPageViews", Sum, Unit::Number),
        (
            "views_per_session",
            t!("Views per session"),
            "screenPageViewsPerSession",
            Average,
            Unit::Number,
        ),
        ("sessions", t!("Sessions"), "sessions", Sum, Unit::Number),
        (
            "engaged_sessions",
            t!("Engaged sessions"),
            "engagedSessions",
            Sum,
            Unit::Number,
        ),
        (
            "engagement_rate",
            t!("Engagement rate"),
            "engagementRate",
            Average,
            Unit::Percent,
        ),
        (
            "bounce_rate",
            t!("Bounce rate"),
            "bounceRate",
            Average,
            Unit::Percent,
        ),
        (
            "session_duration",
            t!("Avg. session duration"),
            "averageSessionDuration",
            Average,
            Unit::Duration,
        ),
        (
            "engagement_time",
            t!("Engagement time"),
            "userEngagementDuration",
            Sum,
            Unit::Duration,
        ),
        (
            "active_users",
            t!("Active users"),
            "activeUsers",
            CountDistinct,
            Unit::Number,
        ),
        (
            "users",
            t!("Total users"),
            "totalUsers",
            CountDistinct,
            Unit::Number,
        ),
        ("new_users", t!("New users"), "newUsers", Sum, Unit::Number),
        ("events", t!("Events"), "eventCount", Sum, Unit::Number),
        (
            "events_per_session",
            t!("Events per session"),
            "eventsPerSession",
            Average,
            Unit::Number,
        ),
        (
            "key_events",
            t!("Key events"),
            "keyEvents",
            Sum,
            Unit::Number,
        ),
        (
            "session_key_event_rate",
            t!("Session key event rate"),
            "sessionKeyEventRate",
            Average,
            Unit::Percent,
        ),
        (
            "user_key_event_rate",
            t!("User key event rate"),
            "userKeyEventRate",
            Average,
            Unit::Percent,
        ),
        (
            "scrolled_users",
            t!("Scrolled users"),
            "scrolledUsers",
            CountDistinct,
            Unit::Number,
        ),
        (
            "search_clicks",
            t!("Search clicks"),
            "organicGoogleSearchClicks",
            Sum,
            Unit::Number,
        ),
        (
            "search_impressions",
            t!("Search impressions"),
            "organicGoogleSearchImpressions",
            Sum,
            Unit::Number,
        ),
        (
            "search_position",
            t!("Search position"),
            "organicGoogleSearchAveragePosition",
            Average,
            Unit::Number,
        ),
    ]
}

/// Whether `field` is a Google API name a caller may pass through: letters,
/// digits and `_`, with an optional `prefix:` (`customEvent:role`).
fn google_field(field: &str) -> bool {
    !field.is_empty()
        && field.len() <= 120
        && field
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
        && field.matches(':').count() <= 1
}

/// The Google Analytics property as a data source.
pub struct AnalyticsSource;

impl AnalyticsSource {
    async fn client(cx: &ReportCx<'_>) -> Result<Analytics, ReportError> {
        let lease = rainmill_vault::lease(cx.db, cx.crypt, SLOT)
            .await
            .map_err(|e| {
                ReportError::Source(format!(
                "{e}. Add a Google Analytics credential under Settings, Credentials, and attach it to Google Analytics reports."
            ))
            })?;
        let key = lease
            .field("key")
            .and_then(SealedFile::parse)
            .ok_or_else(|| {
                ReportError::Source("The Google Analytics credential has no key file".into())
            })?;
        let account = ServiceAccount::parse(&key.bytes).map_err(|e| ReportError::Source(e.0))?;
        Analytics::new(account, lease.field("property_id").unwrap_or_default())
            .map_err(|e| ReportError::Source(e.0))
    }

    fn field_of_dimension(&self, code: &str) -> Option<String> {
        dimension_table()
            .into_iter()
            .find(|(c, _, _)| *c == code)
            .map(|(_, _, f)| f.to_string())
            .or_else(|| self.other_dimension(code).map(|d| d.field))
    }

    fn field_of_metric(&self, code: &str) -> Option<String> {
        metric_table()
            .into_iter()
            .find(|(c, ..)| *c == code)
            .map(|(_, _, f, ..)| f.to_string())
            .or_else(|| self.other_metric(code).map(|m| m.field))
    }
}

/// Google's date field for a group; `None` reports the period as one row.
fn date_field(group: Group) -> Option<&'static str> {
    match group {
        Group::Day => Some("date"),
        Group::Week => Some("isoYearIsoWeek"),
        Group::Month => Some("yearMonth"),
        Group::Year => Some("year"),
        Group::Quarter | Group::Whole => None,
    }
}

/// A Google date value as the first day of its group, never before `from`;
/// `None` for a value that is not a date (`(other)`).
fn date_key(value: &str, group: Group, from: NaiveDate) -> Option<NaiveDate> {
    if !value.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let num = |range: std::ops::Range<usize>| value.get(range)?.parse::<u32>().ok();
    let year = num(0..4)? as i32;
    let date = match group {
        Group::Day => NaiveDate::from_ymd_opt(year, num(4..6)?, num(6..8)?)?,
        Group::Week => NaiveDate::from_isoywd_opt(year, num(4..6)?, chrono::Weekday::Mon)?,
        Group::Month => NaiveDate::from_ymd_opt(year, num(4..6)?, 1)?,
        _ => NaiveDate::from_ymd_opt(year, 1, 1)?,
    };
    Some(date.max(from))
}

fn value_of(cell: &Value) -> Option<f64> {
    cell["value"].as_str().and_then(|v| v.parse::<f64>().ok())
}

fn string_filter(field: &str, value: &str, match_type: &str) -> Value {
    json!({ "filter": { "fieldName": field, "stringFilter": {
        "value": value, "matchType": match_type, "caseSensitive": false } } })
}

/// The `runReport` request for `query` over its whole period, grouped by
/// `dimension` when given.
fn request(query: &ReportQuery, dimension: Option<&str>, date: bool, metrics: &[String]) -> Value {
    let mut body = json!({
        "dateRanges": [{ "startDate": query.from.to_string(), "endDate": query.to.to_string() }],
        "metrics": metrics.iter().map(|m| json!({ "name": m })).collect::<Vec<_>>(),
        "returnPropertyQuota": true,
    });
    if query.totals {
        body["metricAggregations"] = json!(["TOTAL"]);
    }
    let Some(field) = dimension else {
        return body;
    };
    body["dimensions"] = json!([{ "name": field }]);
    let order =
        match &query.order {
            Some(o) => match &o.by {
                OrderBy::Metric(code) => query.metrics.iter().position(|m| m == code).map(
                    |i| json!({ "metric": { "metricName": metrics[i] }, "desc": o.descending }),
                ),
                OrderBy::Dimension => Some(json!({ "dimension": { "dimensionName": field,
                "orderType": if date { "NUMERIC" } else { "CASE_INSENSITIVE_ALPHANUMERIC" } },
                "desc": o.descending })),
            },
            None if date => {
                Some(json!({ "dimension": { "dimensionName": field, "orderType": "NUMERIC" } }))
            }
            None => None,
        };
    if let Some(order) = order {
        body["orderBys"] = json!([order]);
    }
    if !date {
        let mut all: Vec<Value> = query
            .filters
            .iter()
            .map(|f| {
                let first = f.values.first().map(String::as_str).unwrap_or_default();
                match f.op {
                    FilterOp::Equals => string_filter(field, first, "EXACT"),
                    FilterOp::StartsWith => string_filter(field, first, "BEGINS_WITH"),
                    FilterOp::Contains => string_filter(field, first, "CONTAINS"),
                    FilterOp::OneOf => json!({ "filter": { "fieldName": field,
                        "inListFilter": { "values": f.values, "caseSensitive": false } } }),
                }
            })
            .collect();
        if query.hide_empty {
            all.push(json!({ "notExpression": { "orGroup": { "expressions": [
                string_filter(field, NOT_SET, "EXACT"),
                { "filter": { "fieldName": field, "emptyFilter": {} } },
            ] } } }));
        }
        match all.len() {
            0 => {}
            1 => body["dimensionFilter"] = all.remove(0),
            _ => body["dimensionFilter"] = json!({ "andGroup": { "expressions": all } }),
        }
    }
    if let Some(page) = query.page {
        body["limit"] = json!(page.per_page);
        body["offset"] = json!(page.offset());
    } else if let Some(limit) = query.limit {
        body["limit"] = json!(limit);
    }
    body
}

#[laterite_core::strata::async_trait]
impl DataSource for AnalyticsSource {
    fn code(&self) -> &str {
        crate::MODULE_ID
    }

    fn label(&self) -> Text {
        t!("Google Analytics")
    }

    fn permission(&self) -> Option<&str> {
        Some(VIEW)
    }

    fn dimensions(&self) -> Vec<Dimension> {
        let mut all = vec![Dimension::date("date")];
        all.extend(
            dimension_table()
                .into_iter()
                .map(|(c, l, f)| Dimension::new(c, l, f)),
        );
        all
    }

    fn metrics(&self) -> Vec<Metric> {
        metric_table()
            .into_iter()
            .map(|(c, l, f, a, u)| Metric::new(c, l, f).aggregate(a).unit(u))
            .collect()
    }

    /// Any Google field by its API name, `ga:` first (`ga:operatingSystemVersion`),
    /// and the property's custom dimensions (`customEvent:role`).
    fn other_dimension(&self, code: &str) -> Option<Dimension> {
        let field = code.strip_prefix("ga:").unwrap_or(code);
        let custom = field.starts_with("customEvent:") || field.starts_with("customUser:");
        ((code.starts_with("ga:") || custom) && google_field(field))
            .then(|| Dimension::new(code, Text::dynamic(field.to_string()), field))
    }

    /// Any Google metric by its API name, `ga:` first, and per-event key event
    /// counts and rates (`keyEvents:generate_lead`).
    fn other_metric(&self, code: &str) -> Option<Metric> {
        let field = code.strip_prefix("ga:").unwrap_or(code);
        let named = [
            "keyEvents:",
            "sessionKeyEventRate:",
            "userKeyEventRate:",
            "customEvent:",
        ]
        .iter()
        .any(|p| field.starts_with(p));
        if !(code.starts_with("ga:") || named) || !google_field(field) {
            return None;
        }
        let rate = field.contains("Rate");
        Some(
            Metric::new(code, Text::dynamic(field.to_string()), field)
                .aggregate(if rate {
                    Aggregate::Average
                } else {
                    Aggregate::Sum
                })
                .unit(if rate { Unit::Percent } else { Unit::Number }),
        )
    }

    fn cache_for(&self) -> Option<Duration> {
        Some(Duration::from_secs(600))
    }

    async fn fetch(
        &self,
        cx: &ReportCx<'_>,
        query: &ReportQuery,
    ) -> Result<ReportData, ReportError> {
        let metrics: Vec<String> = query
            .metrics
            .iter()
            .map(|code| {
                self.field_of_metric(code)
                    .ok_or_else(|| ReportError::Unsupported(format!("no metric `{code}`")))
            })
            .collect::<Result<_, _>>()?;
        let date = query.dimension == "date";
        let client = Self::client(cx).await?;
        let google = |e: crate::google::GoogleError| ReportError::Source(e.0);

        if date && date_field(query.group).is_none() {
            // One report per quarter (Google has no quarter field), or one for
            // the whole period.
            let mut spans = Vec::new();
            if query.group == Group::Quarter {
                let mut start = query.from;
                while start <= query.to {
                    let next = Group::Quarter.start(start, NaiveDate::MIN) + Months::new(3);
                    let end = (next - chrono::Duration::days(1)).min(query.to);
                    spans.push((start, end));
                    start = next;
                }
            } else {
                spans.push((query.from, query.to));
            }
            let mut rows = Vec::new();
            for (from, to) in spans {
                let mut part = query.clone();
                part.from = from;
                part.to = to;
                part.totals = false;
                let answer = client
                    .run_report(&request(&part, None, true, &metrics))
                    .await
                    .map_err(google)?;
                let values = answer["rows"][0]["metricValues"]
                    .as_array()
                    .map(|cells| cells.iter().map(value_of).collect())
                    .unwrap_or_else(|| vec![Some(0.0); metrics.len()]);
                rows.push(Row {
                    key: from.to_string(),
                    values,
                });
            }
            let totals = if query.totals && query.group == Group::Quarter {
                let answer = client
                    .run_report(&request(query, None, true, &metrics))
                    .await
                    .map_err(google)?;
                cells(&answer["rows"][0]["metricValues"])
            } else {
                rows.first().map(|r| r.values.clone()).unwrap_or_default()
            };
            return Ok(ReportData {
                rows,
                totals,
                total_rows: None,
                grouped: true,
            });
        }

        let field = if date {
            date_field(query.group).map(str::to_string)
        } else {
            self.field_of_dimension(&query.dimension)
        }
        .ok_or_else(|| ReportError::Unsupported(format!("no dimension `{}`", query.dimension)))?;
        let answer = client
            .run_report(&request(query, Some(&field), date, &metrics))
            .await
            .map_err(google)?;
        let rows = answer["rows"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        let raw = row["dimensionValues"][0]["value"]
                            .as_str()
                            .unwrap_or_default();
                        let key = if date {
                            date_key(raw, query.group, query.from)?.to_string()
                        } else {
                            raw.to_string()
                        };
                        Some(Row {
                            key,
                            values: cells(&row["metricValues"]),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let totals = cells(&answer["totals"][0]["metricValues"]);
        let total_rows = query.page.and(answer["rowCount"].as_u64());
        Ok(ReportData {
            rows,
            totals,
            total_rows,
            grouped: true,
        })
    }
}

fn cells(values: &Value) -> Vec<Option<f64>> {
    values
        .as_array()
        .map(|c| c.iter().map(value_of).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn google_dates_become_the_first_day_of_their_group() {
        let from = d("2026-01-01");
        assert_eq!(
            date_key("20261117", Group::Day, from),
            Some(d("2026-11-17"))
        );
        assert_eq!(date_key("202647", Group::Week, from), Some(d("2026-11-16")));
        assert_eq!(
            date_key("202611", Group::Month, from),
            Some(d("2026-11-01"))
        );
        assert_eq!(
            date_key("2026", Group::Year, d("2026-03-05")),
            Some(d("2026-03-05"))
        );
        assert_eq!(date_key("(other)", Group::Day, from), None);
    }

    #[test]
    fn a_query_becomes_a_run_report_request() {
        let mut q = ReportQuery::new(
            "page_path",
            &["views", "active_users"],
            d("2026-11-01"),
            d("2026-11-17"),
        );
        q.order = Some(rainmill_reporting::Order {
            by: OrderBy::Metric("views".into()),
            descending: true,
        });
        q.limit = Some(10);
        q.hide_empty = true;
        q.totals = true;
        q.filters = vec![rainmill_reporting::Filter {
            op: FilterOp::StartsWith,
            values: vec!["/legal".into()],
        }];
        let body = request(
            &q,
            Some("pagePath"),
            false,
            &["screenPageViews".into(), "activeUsers".into()],
        );
        assert_eq!(body["dimensions"][0]["name"], "pagePath");
        assert_eq!(
            body["orderBys"][0]["metric"]["metricName"],
            "screenPageViews"
        );
        assert_eq!(body["orderBys"][0]["desc"], true);
        assert_eq!(body["limit"], 10);
        assert_eq!(body["metricAggregations"][0], "TOTAL");
        let all = &body["dimensionFilter"]["andGroup"]["expressions"];
        assert_eq!(all[0]["filter"]["stringFilter"]["matchType"], "BEGINS_WITH");
        assert_eq!(
            all[1]["notExpression"]["orGroup"]["expressions"][0]["filter"]["stringFilter"]["value"],
            "(not set)"
        );
    }

    #[test]
    fn only_api_names_pass_through() {
        let s = AnalyticsSource;
        assert_eq!(
            s.other_dimension("ga:operatingSystemVersion")
                .unwrap()
                .field,
            "operatingSystemVersion"
        );
        assert_eq!(
            s.other_dimension("customEvent:role").unwrap().field,
            "customEvent:role"
        );
        assert!(s.other_dimension("operatingSystemVersion").is_none());
        assert!(s.other_dimension("ga:pagePath\"}").is_none());
        let m = s.other_metric("sessionKeyEventRate:generate_lead").unwrap();
        assert_eq!((m.aggregate, m.unit), (Aggregate::Average, Unit::Percent));
        assert_eq!(
            s.other_metric("keyEvents:generate_lead").unwrap().aggregate,
            Aggregate::Sum
        );
    }
}
