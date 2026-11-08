use super::*;

#[test]
fn service_counts_and_remembers_last_error() {
    let s = Service::new();
    s.record(200);
    s.record(502);
    let v = s.json("http://x");
    assert_eq!(v["ok"], 1);
    assert_eq!(v["errors"], 1);
    assert_eq!(v["last_error"], "HTTP 502");
    assert!(v["last_ok_ms"].is_number());
}

#[test]
fn fresh_service_reports_no_times() {
    let v = Service::new().json("u");
    assert!(v["last_ok_ms"].is_null());
    assert!(v["last_error_ms"].is_null());
}
