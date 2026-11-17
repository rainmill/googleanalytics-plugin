//! Test connection on the credential's page: the key, the property and the
//! quota, each tried against Google on its own row.

use laterite_core::{t, SealedFile, Text};
use rainmill_vault::action::{Action, ActionCx, Part, PartResult, State};

use crate::google::{Analytics, GoogleError, ServiceAccount};

pub(crate) struct Test;

fn client(cx: &ActionCx<'_>) -> Result<Analytics, GoogleError> {
    let key = cx
        .field("key")
        .and_then(SealedFile::parse)
        .ok_or_else(|| GoogleError("no key file is stored".into()))?;
    let account = ServiceAccount::parse(&key.bytes)?;
    Analytics::new(account, cx.field("property_id").unwrap_or_default())
}

#[laterite_core::strata::async_trait]
impl Action for Test {
    fn parts(&self) -> Vec<Part> {
        vec![
            Part::new("key", t!("Service account key")),
            Part::new("property", t!("Access to the property")),
            Part::new("quota", t!("Today's quota")),
        ]
    }

    fn note(&self) -> Option<Text> {
        Some(t!(
            "Each part tried against Google. Nothing is changed in Google Analytics."
        ))
    }

    async fn part(&self, cx: &ActionCx<'_>, code: &str) -> PartResult {
        let client = match client(cx) {
            Ok(c) => c,
            Err(e) => return PartResult::new(State::Failed, e.0),
        };
        match code {
            "key" => match client.token().await {
                Ok(_) => PartResult::new(
                    State::Works,
                    format!("Google accepted {}", client.account()),
                )
                .accepted(),
                Err(e) => PartResult::new(State::Failed, e.0),
            },
            "property" => match client.metadata().await {
                Ok(meta) => {
                    let count = |k: &str| meta[k].as_array().map_or(0, Vec::len);
                    let custom = meta["dimensions"].as_array().map_or(0, |d| {
                        d.iter().filter(|x| x["customDefinition"] == true).count()
                    });
                    PartResult::new(
                        State::Works,
                        format!(
                            "{} dimensions and {} metrics readable, {custom} of them your custom dimensions",
                            count("dimensions"),
                            count("metrics")
                        ),
                    )
                }
                Err(e) => PartResult::new(
                    State::Failed,
                    format!(
                        "{} Add {} as a Viewer under the property's access management.",
                        e.0,
                        client.account()
                    ),
                ),
            },
            _ => {
                let body = laterite_core::serde_json::json!({
                    "dateRanges": [{ "startDate": "today", "endDate": "today" }],
                    "metrics": [{ "name": "sessions" }],
                    "returnPropertyQuota": true,
                });
                match client.run_report(&body).await {
                    Ok(answer) => {
                        let day = &answer["propertyQuota"]["tokensPerDay"];
                        PartResult::new(
                            State::Works,
                            format!(
                                "{} of today's tokens left; this check used {}",
                                day["remaining"].as_i64().unwrap_or_default(),
                                day["consumed"].as_i64().unwrap_or_default()
                            ),
                        )
                    }
                    Err(e) => PartResult::new(State::Failed, e.0),
                }
            }
        }
    }
}
