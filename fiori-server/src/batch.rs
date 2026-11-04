use axum::{
    Router,
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use std::collections::{HashMap, HashSet};
use tower::ServiceExt;

use crate::{AppState, sticky};

pub fn router(inner: Router<AppState>, state: AppState) -> Router<AppState> {
    let target = inner.clone().with_state(state.clone());
    inner.route(
        "/$batch",
        post(move |headers: HeaderMap, body: Bytes| {
            let target = target.clone();
            let state = state.clone();
            async move {
                match handle_batch(target, &state, headers, body).await {
                    Ok(response) => response,
                    Err(response) => response,
                }
            }
        }),
    )
}

fn bad_request(message: impl Into<String>) -> Response {
    sticky::error(StatusCode::BAD_REQUEST, message)
}

fn extract_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| {
            part.strip_prefix("boundary=")
                .map(|b| b.trim_matches('"').to_string())
        })
        .filter(|boundary| !boundary.is_empty())
}

struct Part {
    headers: HeaderMap,
    bytes: Bytes,
}

async fn parts(content_type: &str, body: Bytes) -> Result<Vec<Part>, Response> {
    let boundary =
        extract_boundary(content_type).ok_or_else(|| bad_request("Missing multipart boundary"))?;
    let stream = futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut result = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        let headers = field.headers().clone();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| bad_request(e.to_string()))?;
        result.push(Part { headers, bytes });
    }
    Ok(result)
}

fn parse_raw_http_request(raw: &[u8]) -> Result<Request, Response> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
        .unwrap_or(raw.len());
    let text =
        std::str::from_utf8(&raw[..split]).map_err(|_| bad_request("Invalid HTTP headers"))?;
    let mut lines = text.lines();
    let line = lines
        .next()
        .ok_or_else(|| bad_request("Missing HTTP request line"))?;
    let mut words = line.split_whitespace();
    let method = Method::from_bytes(words.next().unwrap_or("").as_bytes())
        .map_err(|_| bad_request("Invalid HTTP method"))?;
    let path = words
        .next()
        .ok_or_else(|| bad_request("Missing HTTP path"))?;
    if !matches!(words.next(), Some("HTTP/1.0" | "HTTP/1.1")) || words.next().is_some() {
        return Err(bad_request("Invalid HTTP request line"));
    }
    // Both service-relative and service-rooted URLs occur in UI5 batches.
    let path = path
        .strip_prefix("/Service/")
        .unwrap_or(path)
        .trim_start_matches('/');
    let mut builder = Request::builder().method(method).uri(format!("/{path}"));
    for line in lines.filter(|line| !line.is_empty()) {
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| bad_request("Invalid HTTP header"))?;
        builder = builder.header(key.trim(), value.trim());
    }
    builder
        .body(Body::from(raw[split..].to_vec()))
        .map_err(|e| bad_request(e.to_string()))
}

async fn response_part(response: Response, content_id: Option<&HeaderValue>) -> Vec<u8> {
    let status = response.status();
    let headers = response.headers().clone();
    let mut part =
        b"Content-Type: application/http\r\nContent-Transfer-Encoding: binary\r\n".to_vec();
    if let Some(id) = content_id {
        part.extend_from_slice(b"Content-ID: ");
        part.extend_from_slice(id.as_bytes());
        part.extend_from_slice(b"\r\n");
    }
    part.extend_from_slice(
        format!(
            "\r\nHTTP/1.1 {} {}\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        )
        .as_bytes(),
    );
    for (name, value) in &headers {
        part.extend_from_slice(name.as_str().as_bytes());
        part.extend_from_slice(b": ");
        part.extend_from_slice(value.as_bytes());
        part.extend_from_slice(b"\r\n");
    }
    part.extend_from_slice(b"\r\n");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("in-memory OData response body");
    part.extend_from_slice(&body);
    part
}

fn multipart(parts: Vec<Vec<u8>>, boundary: &str) -> Vec<u8> {
    let mut result = Vec::new();
    for part in parts {
        result.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        result.extend_from_slice(&part);
        result.extend_from_slice(b"\r\n");
    }
    result.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    result
}

async fn dispatch(
    router: &Router,
    mut request: Request,
    context: &mut Option<HeaderValue>,
) -> Response {
    // The outer context is authoritative; subrequests cannot switch sessions.
    request.headers_mut().remove(sticky::CONTEXT);
    if let Some(id) = context.as_ref() {
        request.headers_mut().insert(sticky::CONTEXT, id.clone());
    }
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("infallible router");
    *context = response.headers().get(sticky::CONTEXT).cloned();
    response
}

fn resolve_reference(
    request: &mut Request,
    references: &HashMap<String, String>,
) -> Result<(), Response> {
    let path = request.uri().to_string();
    if let Some(reference) = path.strip_prefix("/$") {
        let end = reference.find(['/', '?']).unwrap_or(reference.len());
        let location = references
            .get(&reference[..end])
            .ok_or_else(|| bad_request("Unknown or forward Content-ID reference"))?;
        let location = location.strip_prefix("/Service").unwrap_or(location);
        *request.uri_mut() = format!("{location}{}", &reference[end..])
            .parse()
            .map_err(|_| bad_request("Invalid Content-ID reference URI"))?;
    }
    Ok(())
}

async fn handle_batch(
    router: Router,
    state: &AppState,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| bad_request("Missing Content-Type"))?;
    let incoming = parts(content_type, body).await?;
    let mut context = headers.get(sticky::CONTEXT).cloned();
    if let Some(id) = &context {
        let id = id
            .to_str()
            .map_err(|_| bad_request("Invalid sap-contextid"))?;
        match &state.sticky {
            Some(sessions) => sessions.validate_context(id)?,
            None => return Err(bad_request("Sticky sessions are not enabled")),
        }
    }
    let mut responses = Vec::new();
    for part in incoming {
        let content_type = part
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| bad_request("Missing part Content-Type"))?;
        if content_type.starts_with("multipart/mixed") {
            let changes = parts(content_type, part.bytes).await?;
            if changes.is_empty() {
                return Err(bad_request("An empty changeset is not allowed"));
            }
            if state.sticky.is_none() && changes.len() > 1 {
                return Err(sticky::error(
                    StatusCode::NOT_IMPLEMENTED,
                    "Atomic multi-operation changesets currently require the designer store",
                ));
            }
            let ids: Vec<_> = changes
                .iter()
                .map(|p| p.headers.get("content-id").cloned())
                .collect();
            let mut seen = HashSet::new();
            for id in ids.iter().flatten() {
                let id = id.to_str().map_err(|_| bad_request("Invalid Content-ID"))?;
                if id.is_empty() || !seen.insert(id) {
                    return Err(bad_request(
                        "Content-ID must be nonempty and unique within a changeset",
                    ));
                }
            }
            let mut requests = Vec::new();
            for change in changes {
                let request = parse_raw_http_request(&change.bytes)?;
                if request.method() == Method::GET || request.method() == Method::HEAD {
                    return Err(bad_request("Read requests are not allowed in a changeset"));
                }
                requests.push(request);
            }
            let initial = context.clone();
            let execute = || async {
                let mut responses = Vec::new();
                let mut references = HashMap::new();
                for (mut request, id) in requests.into_iter().zip(&ids) {
                    if let Err(response) = resolve_reference(&mut request, &references) {
                        responses.push(response);
                        break;
                    }
                    let response = dispatch(&router, request, &mut context).await;
                    let failed = !response.status().is_success();
                    if !failed {
                        if let (Some(id), Some(location)) =
                            (id, response.headers().get(header::LOCATION))
                        {
                            if let Ok(location) = location.to_str() {
                                references.insert(
                                    id.to_str().expect("validated Content-ID").to_string(),
                                    location.to_string(),
                                );
                            }
                        }
                    }
                    responses.push(response);
                    if failed {
                        break;
                    }
                }
                responses
            };
            let result = if let Some(sessions) = &state.sticky {
                sessions.changeset(execute).await
            } else {
                Ok(execute().await)
            };
            let result = match result {
                Ok(result) => result,
                Err(response) => {
                    context = initial;
                    responses.push(response_part(response, None).await);
                    continue;
                }
            };
            if result.iter().any(|r| !r.status().is_success()) {
                context = initial;
                let failure = result
                    .into_iter()
                    .find(|r| !r.status().is_success())
                    .expect("failed response");
                responses.push(response_part(failure, None).await);
            } else {
                let mut nested = Vec::new();
                for (response, id) in result.into_iter().zip(&ids) {
                    nested.push(response_part(response, id.as_ref()).await);
                }
                let mut part =
                    b"Content-Type: multipart/mixed; boundary=changeset_response\r\n\r\n".to_vec();
                part.extend_from_slice(&multipart(nested, "changeset_response"));
                responses.push(part);
            }
        } else if content_type.starts_with("application/http") {
            let request = parse_raw_http_request(&part.bytes)?;
            let response = dispatch(&router, request, &mut context).await;
            responses.push(response_part(response, part.headers.get("content-id")).await);
        } else {
            return Err(bad_request("Unsupported batch part Content-Type"));
        }
    }
    let mut response = (
        [(
            header::CONTENT_TYPE,
            "multipart/mixed; boundary=batch_response",
        )],
        multipart(responses, "batch_response"),
    )
        .into_response();
    response.headers_mut().insert(
        HeaderName::from_static("odata-version"),
        HeaderValue::from_static("4.0"),
    );
    if context.as_ref().is_some_and(|id| {
        state.sticky.as_ref().is_some_and(|sessions| {
            id.to_str()
                .ok()
                .is_some_and(|id| sessions.validate_context(id).is_err())
        })
    }) {
        context = None;
    }
    if let Some(context) = context {
        response.headers_mut().insert(sticky::CONTEXT, context);
        response
            .headers_mut()
            .insert(sticky::TIMEOUT, HeaderValue::from_static("120"));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn raw_requests_preserve_binary_bodies_and_normalize_service_paths() {
        let raw = b"POST /Service/EntitySet HTTP/1.1\r\nContent-Type: application/octet-stream\r\n\r\n\xff\0";
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
}
