# Rainmill Google Analytics

Google Analytics 4 for a [Laterite](https://laterite.rs) application.

- **Reports**: the property's traffic, pages, sources, devices, places and events on the admin's report dashboards, through [Rainmill Reporting](https://github.com/rainmill/reporting-plugin), with a ready-made "Google Analytics" dashboard.
- **Tracker**: a theme component that puts the Google tag on the site, with consent left to the site.

## Requirements

Laterite 0.11, `rainmill-vault` and `rainmill-reporting`. A Google Analytics 4 property, and a Google Cloud service account with the Google Analytics Data API enabled, added as a Viewer under the property's access management.

## Installation

```toml
[dependencies]
rainmill-googleanalytics = "0.1"
```

Add `rainmill_googleanalytics::module()` to the application's modules, after `rainmill_vault::module()` and `rainmill_reporting::module()`.

## Store the credential

Under Settings, Credentials, add a **Google Analytics** credential and attach it to **Google Analytics reports**. Test connection tries the key, the property and today's quota.

| Field | Where it comes from |
| --- | --- |
| Service account key | Google Cloud, IAM, Service accounts, the account, Keys, Add key, JSON |
| Property ID | Google Analytics, Admin, Property settings |

## Reports

The data source is `rainmill.googleanalytics`.

| Dimension | Google field |
| --- | --- |
| `date` | `date`, `isoYearIsoWeek`, `yearMonth`, `year` |
| `page_path`, `page_title`, `landing_page`, `hostname` | `pagePath`, `pageTitle`, `landingPage`, `hostName` |
| `source`, `medium`, `source_medium`, `channel`, `campaign`, `referrer` | `sessionSource`, `sessionMedium`, `sessionSourceMedium`, `sessionDefaultChannelGroup`, `sessionCampaignName`, `pageReferrer` |
| `first_source`, `first_channel` | `firstUserSource`, `firstUserDefaultChannelGroup` |
| `country`, `region`, `city`, `language` | `country`, `region`, `city`, `language` |
| `device`, `os`, `browser`, `screen` | `deviceCategory`, `operatingSystem`, `browser`, `screenResolution` |
| `new_or_returning`, `event`, `key_event` | `newVsReturning`, `eventName`, `isKeyEvent` |
| `weekday`, `hour`, `outbound`, `file`, `search_term` | `dayOfWeekName`, `hour`, `linkDomain`, `fileName`, `searchTerm` |
| `customEvent:<param>`, `customUser:<param>` | The property's custom dimensions |
| `ga:<field>` | Any other Google dimension by its API name |

| Metric | Google field |
| --- | --- |
| `views`, `views_per_session` | `screenPageViews`, `screenPageViewsPerSession` |
| `sessions`, `engaged_sessions`, `engagement_rate`, `bounce_rate` | `sessions`, `engagedSessions`, `engagementRate`, `bounceRate` |
| `session_duration`, `engagement_time` | `averageSessionDuration`, `userEngagementDuration` |
| `active_users`, `users`, `new_users`, `scrolled_users` | `activeUsers`, `totalUsers`, `newUsers`, `scrolledUsers` |
| `events`, `events_per_session`, `key_events` | `eventCount`, `eventsPerSession`, `keyEvents` |
| `session_key_event_rate`, `user_key_event_rate` | `sessionKeyEventRate`, `userKeyEventRate` |
| `search_clicks`, `search_impressions`, `search_position` | `organicGoogleSearch*` (needs a Search Console link) |
| `keyEvents:<event>`, `sessionKeyEventRate:<event>`, `customEvent:<param>` | One key event, or a custom metric |
| `ga:<field>` | Any other Google metric by its API name |

```yaml
signups:
  type: chart
  chart: bar
  data_source: rainmill.googleanalytics
  metrics: [keyEvents:generate_lead]
```

Answers are kept for 10 minutes; Refresh on the dashboard asks again.

## Tracker

```yaml
---
components:
  analytics: { component: rainmill.googleanalytics.tracker, measurement_id: G-XXXXXXXXXX, consent: wait, remember: true }
---
```

```jinja
<head>
  {{ component("analytics") }}
</head>
```

| Property | Default | Description |
| --- | --- | --- |
| `measurement_id` | | The `G-` ID of the web stream |
| `tag_id` | | A `GTM-` Tag Manager ID, loaded in place of the Google tag |
| `consent` | `none` | `none`, `wait` or `mode` |
| `remember` | `false` | Keep the visitor's answer in the browser |
| `regions` | | ISO codes the `mode` default applies to (`DE,FR,US-CA`); every visitor when empty |
| `cookie_domain` | | Passed to Google as `cookie_domain` |
| `storage_key` | `lat-analytics` | The browser storage key `remember` keeps the answer under |
| `in_preview` | `false` | Load while an operator previews the theme |

| `consent` | The tag |
| --- | --- |
| `none` | Loads on every page |
| `wait` | Loads after `latAnalytics.grant()` |
| `mode` | Loads at once under Google Consent Mode v2 with every signal denied; `grant()` grants them |

The script is a file at `/rainmill/googleanalytics/tracker.js`. A site with a content security policy allows `https://www.googletagmanager.com` in `script-src`.

### The consent seam

```js
latAnalytics.grant();               // statistics allowed
latAnalytics.grant({ ads: true });  // also ad storage and personalisation (mode)
latAnalytics.deny();                // refused; the _ga cookies are removed
latAnalytics.state();               // "granted", "denied" or "unset"
latAnalytics.event("signup", { plan: "free" });
latAnalytics.ask();                 // fires lat:analytics with ask: true
document.addEventListener("lat:analytics", (e) => e.detail); // { state, ask }
```

| Attribute | On click |
| --- | --- |
| `data-analytics-grant` | `grant()`; `data-analytics-grant="ads"` also grants ad signals |
| `data-analytics-deny` | `deny()` |
| `data-analytics-ask` | `ask()` |
| `data-track="<event>"` | `event("<event>", { <param>: value })` from each `data-track-<param>` |

`lat:analytics` fires once when the visitor has not answered, and on every change. An event sent before an answer waits, and is dropped on `deny()`.

### A banner

```html
<div class="consent" id="consent" hidden>
  <p>May we count your visit with Google Analytics? <a href="/privacy">Privacy</a></p>
  <button data-analytics-deny>No thanks</button>
  <button data-analytics-grant>Allow</button>
</div>
<button data-analytics-ask>Cookie choice</button>
<script>
  document.addEventListener("lat:analytics", (e) => {
    document.getElementById("consent").hidden = !(e.detail.state === "unset" || e.detail.ask);
  });
</script>
```

Load the banner's script after the tracker, or as a file with `defer`.

## When Google is unavailable

A revoked key, a spent quota or an outage shows on the dashboard as one notice above the Google widgets; the rest of the dashboard renders. The tracker failing to load changes nothing for visitors.

## License

MIT or Apache-2.0, at your option.
