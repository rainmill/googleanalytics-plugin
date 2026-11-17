//! The tracker component: the Google tag on the site's pages, with the consent
//! choice left to the site. The script is served as a file, so a site whose
//! script policy forbids inline script needs only to allow Google's host.

use std::sync::Arc;

use laterite_admin::axum::http::header;
use laterite_admin::axum::response::IntoResponse;
use laterite_admin::axum::routing::get;
use laterite_admin::axum::Router;
use laterite_admin::routes::{PublicRoute, RouteCtx};
use laterite_core::serde_json::{json, Map, Value};
use laterite_web::theme::component::{
    Component, ComponentContext, ComponentOutput, PropertyDef, PropertyKind,
};
use laterite_web::theme::ThemeError;

/// The component's code.
pub const COMPONENT: &str = "rainmill.googleanalytics.tracker";
/// Where the script is served.
pub const SCRIPT_PATH: &str = "/rainmill/googleanalytics/tracker.js";

const SCRIPT: &str = include_str!("../assets/tracker.js");

/// A short digest of the script, so a changed script is fetched again.
fn version() -> String {
    let mut hash: u32 = 2166136261;
    for b in SCRIPT.bytes() {
        hash = (hash ^ u32::from(b)).wrapping_mul(16777619);
    }
    format!("{hash:08x}")
}

const MARKUP: &str = r#"{% set t = component %}{% if t.enabled %}<script src="{{ t.src }}" data-lat-measurement-id="{{ t.measurement_id }}" data-lat-tag-id="{{ t.tag_id }}" data-lat-consent="{{ t.consent }}" data-lat-remember="{{ 'true' if t.remember else 'false' }}" data-lat-regions="{{ t.regions }}" data-lat-cookie-domain="{{ t.cookie_domain }}" data-lat-storage-key="{{ t.storage_key }}"></script>{% endif %}"#;

/// The consent settings the component accepts.
const CONSENTS: &[&str] = &["none", "wait", "mode"];

pub(crate) struct Tracker;

#[laterite_core::strata::async_trait]
impl Component for Tracker {
    fn properties(&self) -> Vec<PropertyDef> {
        vec![
            PropertyDef::new("measurement_id", "Measurement ID (G-…)", PropertyKind::Text),
            PropertyDef::new("tag_id", "Tag Manager ID (GTM-…)", PropertyKind::Text),
            PropertyDef::new("consent", "Consent: none, wait or mode", PropertyKind::Text)
                .default("none"),
            PropertyDef::new(
                "remember",
                "Remember the answer in the browser",
                PropertyKind::Bool,
            )
            .default(false),
            PropertyDef::new(
                "regions",
                "Consent mode regions (ISO codes, comma separated)",
                PropertyKind::Text,
            ),
            PropertyDef::new("cookie_domain", "Cookie domain", PropertyKind::Text),
            PropertyDef::new(
                "storage_key",
                "Where the answer is kept in the browser",
                PropertyKind::Text,
            )
            .default("lat-analytics"),
            PropertyDef::new(
                "in_preview",
                "Load while an operator previews",
                PropertyKind::Bool,
            )
            .default(false),
        ]
    }

    fn markup(&self) -> Option<&'static str> {
        Some(MARKUP)
    }

    async fn run(&self, cx: &ComponentContext<'_>) -> Result<ComponentOutput, ThemeError> {
        Ok(ComponentOutput::data(settings(cx.properties, cx.preview)))
    }
}

/// What the markup renders from, with every value checked: an ID is letters,
/// digits and `-`, consent one of [`CONSENTS`], regions two-letter codes.
fn settings(properties: &Map<String, Value>, preview: bool) -> Value {
    let text = |k: &str| {
        properties
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
    };
    let id = |k: &str| {
        let v = text(k);
        if v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            v.to_string()
        } else {
            String::new()
        }
    };
    let flag = |k: &str| properties.get(k).and_then(Value::as_bool).unwrap_or(false);
    let (measurement_id, tag_id) = (id("measurement_id"), id("tag_id"));
    let consent = Some(text("consent"))
        .filter(|c| CONSENTS.contains(c))
        .unwrap_or("none");
    let regions: Vec<String> = text("regions")
        .split(',')
        .map(|r| r.trim().to_ascii_uppercase())
        .filter(|r| r.len() == 2 || (r.len() == 5 && r.as_bytes()[2] == b'-'))
        .filter(|r| r.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .collect();
    let cookie_domain: String = text("cookie_domain")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
        .collect();
    let storage_key: String = Some(text("storage_key"))
        .filter(|k| {
            !k.is_empty()
                && k.len() <= 64
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .unwrap_or("lat-analytics")
        .to_string();
    let enabled =
        !(measurement_id.is_empty() && tag_id.is_empty()) && (!preview || flag("in_preview"));
    json!({
        "enabled": enabled,
        "src": format!("{SCRIPT_PATH}?v={}", version()),
        "measurement_id": measurement_id,
        "tag_id": tag_id,
        "consent": consent,
        "remember": flag("remember"),
        "regions": regions.join(","),
        "cookie_domain": cookie_domain,
        "storage_key": storage_key,
    })
}

/// Serves the script.
pub(crate) struct Script;

impl PublicRoute for Script {
    fn mount(&self, _ctx: &RouteCtx) -> Router {
        Router::new().route(
            "/",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                        (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                    ],
                    SCRIPT,
                )
                    .into_response()
            }),
        )
    }
}

pub(crate) fn script() -> Arc<Script> {
    Arc::new(Script)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn settings_keep_only_well_formed_values() {
        let s = settings(
            &props(
                json!({ "measurement_id": "G-7ZHVWW03R8", "consent": "wait", "remember": true,
                "regions": "in, de, bad region, US-CA", "cookie_domain": "acme.com\"><x" }),
            ),
            false,
        );
        assert_eq!(s["enabled"], true);
        assert_eq!(s["consent"], "wait");
        assert_eq!(s["regions"], "IN,DE,US-CA");
        assert_eq!(s["cookie_domain"], "acme.comx");
        assert_eq!(s["storage_key"], "lat-analytics");
        let s = settings(
            &props(json!({ "measurement_id": "G-1\"onload=x", "consent": "maybe" })),
            false,
        );
        assert_eq!(
            (s["enabled"].clone(), s["consent"].clone()),
            (json!(false), json!("none"))
        );
    }

    #[test]
    fn previews_skip_the_tag_unless_asked() {
        let p = props(json!({ "tag_id": "GTM-TXJWQKMZ" }));
        assert_eq!(settings(&p, true)["enabled"], false);
        let mut p = p;
        p.insert("in_preview".into(), json!(true));
        assert_eq!(settings(&p, true)["enabled"], true);
    }
}
