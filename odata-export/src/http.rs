//! Minimaler HTTP/1.1-GET-Client ueber `std::net`, nur fuer `http://`.
//!
//! Reicht fuer den Export: `Connection: close`, Antwort mit Content-Length oder
//! Transfer-Encoding: chunked. Keine TLS-Unterstuetzung, keine Weiterleitungen.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

pub struct Client {
    host_port: String,
    /// Pfad der Service-Wurzel ohne abschliessenden Schraegstrich, z. B. `/Service`.
    prefix: String,
}

impl Client {
    /// Akzeptiert `http://host:port/Service`.
    pub fn new(base_url: &str) -> Result<Self> {
        let rest = base_url
            .strip_prefix("http://")
            .with_context(|| format!("nur http:// wird unterstuetzt: {base_url}"))?;
        let (host_port, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        Ok(Self {
            host_port: host_port.to_string(),
            prefix: path.trim_end_matches('/').to_string(),
        })
    }

    /// GET auf `prefix + path`. Liefert Statuscode und Rumpf.
    pub fn get(&self, path: &str) -> Result<(u16, Vec<u8>)> {
        self.request("GET", path, None)
    }

    /// Sendet `method` mit optionalem JSON-Rumpf auf `prefix + path`.
    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Vec<u8>)> {
        let target = format!("{}{}", self.prefix, path);
        let mut stream = TcpStream::connect(&self.host_port)
            .with_context(|| format!("Verbindung zu {} fehlgeschlagen", self.host_port))?;
        stream.set_read_timeout(Some(Duration::from_secs(120)))?;
        let payload = match body {
            Some(v) => serde_json::to_vec(v)?,
            None => Vec::new(),
        };
        let mut request = format!(
            "{method} {target} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n",
            self.host_port
        );
        if body.is_some() {
            request.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                payload.len()
            ));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes())?;
        stream.write_all(&payload)?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        parse_response(&raw).with_context(|| format!("{method} {target}"))
    }
}

fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>)> {
    let split = find(raw, b"\r\n\r\n").context("ungueltige HTTP-Antwort (kein Header-Ende)")?;
    let head = std::str::from_utf8(&raw[..split]).context("Header nicht UTF-8")?;
    let body = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .with_context(|| format!("ungueltige Statuszeile: {status_line}"))?;
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }
    let body = if chunked { dechunk(body)? } else { body.to_vec() };
    Ok((status, body))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = find(data, b"\r\n").context("Chunk ohne Groessenzeile")?;
        let size_text = std::str::from_utf8(&data[..line_end])?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .context("ungueltige Chunk-Groesse")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            bail!("Chunk abgeschnitten");
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
