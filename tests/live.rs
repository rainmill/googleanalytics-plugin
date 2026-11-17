//! Against the real Data API, only when a key file and property are named:
//! `GA_KEY_FILE=… GA_PROPERTY=… cargo test -p rainmill-googleanalytics --test live -- --ignored`.

use rainmill_googleanalytics::google::{Analytics, ServiceAccount};

#[tokio::test]
#[ignore = "needs GA_KEY_FILE and GA_PROPERTY"]
async fn a_report_runs_against_the_property() {
    let key = std::fs::read(std::env::var("GA_KEY_FILE").unwrap()).unwrap();
    let client = Analytics::new(
        ServiceAccount::parse(&key).unwrap(),
        &std::env::var("GA_PROPERTY").unwrap(),
    )
    .unwrap();
    let meta = client.metadata().await.unwrap();
    assert!(meta["dimensions"].as_array().unwrap().len() > 100);
    let body = laterite_core::serde_json::json!({
        "dateRanges": [{ "startDate": "28daysAgo", "endDate": "today" }],
        "dimensions": [{ "name": "date" }],
        "metrics": [{ "name": "screenPageViews" }, { "name": "activeUsers" }],
        "metricAggregations": ["TOTAL"],
        "returnPropertyQuota": true,
    });
    let answer = client.run_report(&body).await.unwrap();
    println!(
        "rows {} totals {}",
        answer["rowCount"], answer["totals"][0]["metricValues"]
    );
    assert!(
        answer["propertyQuota"]["tokensPerDay"]["remaining"]
            .as_i64()
            .unwrap()
            > 0
    );
}
