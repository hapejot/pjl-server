use super::*;

#[tokio::test]
async fn raw_requests_preserve_binary_bodies_and_normalize_service_paths() {
    let raw =
        b"POST /Service/EntitySet HTTP/1.1\r\nContent-Type: application/octet-stream\r\n\r\n\xff\0";
    let request = parse_raw_http_request(raw).unwrap();
    assert_eq!(request.uri(), "/EntitySet");
    assert_eq!(request.method(), Method::POST);
    assert_eq!(
        axum::body::to_bytes(request.into_body(), usize::MAX)
            .await
            .unwrap()
            .as_ref(),
        b"\xff\0"
    );
}

#[test]
fn malformed_requests_and_empty_boundaries_are_rejected() {
    for raw in [
        b"".as_slice(),
        b"GET",
        b"PATCH /EntitySet",
        b"GET / HTTP/1.1 extra",
        b"GET / HTTP/1.1\r\nBadHeader\r\n\r\n",
    ] {
        assert!(parse_raw_http_request(raw).is_err());
    }
    assert_eq!(
        extract_boundary("multipart/mixed; boundary=\"example\"").as_deref(),
        Some("example")
    );
    assert!(extract_boundary("multipart/mixed; boundary=\"\"").is_none());
}
