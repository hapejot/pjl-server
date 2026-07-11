use axum::{
    Router,
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, Method, header},
    response::{IntoResponse, Response},
    routing::post,
};
use multer::bytes::BufMut;
use tower::ServiceExt;
use tracing::info;

pub fn router(inner: Router) -> Router {
    let dispatch_target = inner.clone();

    inner.route(
        "/$batch",
        post(move |headers: HeaderMap, body: Bytes| {
            let router = dispatch_target.clone();
            async move { handle_batch(router, headers, body).await }
        }),
    )
}

/// The boundary identified by `boundary=` is extracte and returned if possible.
fn extract_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary="))
        .map(|b| {
            // Boundary kann in Anführungszeichen stehen: boundary="batch_abc123"
            b.trim_matches('"').to_string()
        })
}

/// convert the binary sequence into a request containing a body.
fn parse_raw_http_request(raw: &[u8]) -> Request<Body> {
    // Request-Line und Header sind garantiert ASCII/UTF-8 (laut HTTP-Spec),
    // der Body dahinter aber potenziell binär - daher nur den Header-Teil als str behandeln.
    let header_end = find_header_body_split(raw);
    let header_bytes = &raw[..header_end];
    let body_bytes = &raw[header_end..];

    let header_str = std::str::from_utf8(header_bytes)
        .expect("HTTP-Header müssen laut Spezifikation gültiges ASCII/UTF-8 sein");

    let mut lines = header_str.lines();

    let request_line = lines.next().unwrap();
    let mut parts = request_line.split_whitespace();
    let method = Method::from_bytes(parts.next().unwrap().as_bytes()).unwrap();
    let path = parts.next().unwrap();

    let mut builder = Request::builder()
        .method(method)
        .uri(format!("/{path}").replace("//", "/"));

    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            builder = builder.header(key.trim(), value.trim());
        }
    }

    builder.body(Body::from(body_bytes.to_vec())).unwrap()
}

/// locate position of \r\n\r\n (w. \n\n) separator between header lines and body.
/// question is, do we need the single line ending? There will be no separator if
/// there is no body.
/// In this case the returned length is the full size.
fn find_header_body_split(raw: &[u8]) -> usize {
    raw.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|pos| pos + 4)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|pos| pos + 2))
        .unwrap_or(raw.len())
}

/// convert the response to a binary representation
async fn response_to_batch_part(response: Response) -> Vec<u8> {
    let status = response.status();
    let headers = response.headers().clone();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();

    let mut part = Vec::new();

    part.put_slice(b"Content-Type: application/http\r\n");
    part.put_slice(b"Content-Transfer-Encoding: binary\r\n\r\n");

    part.put_slice(
        format!(
            "HTTP/1.1 {} {}\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        )
        .as_bytes(),
    );

    for (name, value) in headers.iter() {
        part.put_slice(name.as_str().as_bytes());
        part.put_slice(b": ");
        part.put_slice(value.as_bytes()); // .as_bytes() statt .to_str() - auch Header-Values können non-UTF8 sein
        part.put_slice(b"\r\n");
    }

    part.put_slice(b"\r\n");
    part.put_slice(&body_bytes); // rohe Bytes, kein String-Umweg

    part
}

/// generate a full response of the parts
fn build_multipart_response(parts: Vec<Vec<u8>>) -> impl IntoResponse {
    let boundary = "batch_response_boundary";
    let mut body = Vec::new();

    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(&part);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--").as_bytes());

    (
        [(
            header::CONTENT_TYPE,
            format!("multipart/mixed; boundary={boundary}"),
        )],
        body,
    )
}

///
async fn handle_batch(router: Router, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap();

    let boundary = extract_boundary(content_type).unwrap();

    let mut multipart = multer::Multipart::new(
        futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) }),
        boundary,
    );

    let mut response_parts = Vec::new();

    while let Some(field) = multipart.next_field().await.unwrap() {
        let part_bytes = field.bytes().await.unwrap();
        let sub_request = parse_raw_http_request(&part_bytes);
        info!("batch request: {}", sub_request.uri());
        let response = router.clone().oneshot(sub_request).await.unwrap();
        response_parts.push(response_to_batch_part(response).await);
    }

    build_multipart_response(response_parts)
}
