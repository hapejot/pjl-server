#[derive(Debug, Clone, PartialEq)]
pub struct ODataPath {
    pub segments: Vec<PathSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathSegment {
    pub name: String,
    pub keys: Vec<KeyValue>, // leer, falls keine Klammer vorhanden
}
impl PathSegment {
    pub fn is_active_entity(&self) -> bool {
        self.keys.iter().any(|x| {
            x.name.as_deref() == Some("IsActiveEntity")
                && match x.value {
                    KeyLiteral::String(_) => todo!(),
                    KeyLiteral::Number(n) => n == 1.0,
                    KeyLiteral::Guid(_) => todo!(),
                    KeyLiteral::Bool(b) => b,
                }
        })
    }

    pub fn keys_as_map(&self) -> std::collections::BTreeMap<String, String> {
        self.keys
            .iter()
            .filter_map(|k| {
                if k.name.as_deref() != Some("IsActiveEntity") {
                    k.name.clone().map(|name| (name, k.value.to_string_value()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn id(&self) -> Option<Uuid> {
        self.keys
            .iter()
            .find(|x| x.name == Some("ID".into()))
            .and_then(|x| Uuid::parse_str(&x.value.to_string_value()).ok())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyValue {
    pub name: Option<String>, // None bei unbenanntem Einzelschlüssel: EntityConfigs('abc')
    pub value: KeyLiteral,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyLiteral {
    String(String),
    Number(f64),
    Guid(String),
    Bool(bool),
}

impl KeyLiteral {
    pub fn to_string_value(&self) -> String {
        match self {
            KeyLiteral::String(s) => s.clone(),
            KeyLiteral::Number(n) => n.to_string(),
            KeyLiteral::Guid(g) => g.clone(),
            KeyLiteral::Bool(b) => b.to_string(),
        }
    }
}

use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

#[derive(Debug)]
pub struct PathParseError {
    pub segment: String,
    pub message: String,
}

// Ein Segment ist: Name, optional gefolgt von (schluessel[,schluessel...])
static SEGMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<name>[A-Za-z_$][A-Za-z0-9_.]*)(?:\((?P<keys>.*)\))?$")
        .expect("gültige Segment-Regex")
});

// Ein einzelner Key: entweder "name=wert" oder nur "wert"
static KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?:(?P<key>[A-Za-z_][A-Za-z0-9_]*)=)?",
        r"(?:'(?P<string>(?:[^']|'')*)'",
        r"|(?P<guid>[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})",
        r"|(?P<bool>true|false)",
        r"|(?P<number>-?\d+(?:\.\d+)?))$",
    ))
    .expect("gültige Key-Regex")
});

pub fn parse_odata_path(path: &str) -> Result<ODataPath, PathParseError> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(ODataPath { segments: vec![] });
    }

    let segments = trimmed
        .split('/')
        .map(parse_segment)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ODataPath { segments })
}

fn parse_segment(raw: &str) -> Result<PathSegment, PathParseError> {
    let caps = SEGMENT_RE.captures(raw).ok_or_else(|| PathParseError {
        segment: raw.to_string(),
        message: "ungültiges Segment-Format".to_string(),
    })?;

    let name = caps["name"].to_string();

    let keys = match caps.name("keys") {
        Some(m) if !m.as_str().is_empty() => split_keys(m.as_str())
            .iter()
            .map(|k| parse_key(k, raw))
            .collect::<Result<Vec<_>, _>>()?,
        _ => vec![],
    };

    Ok(PathSegment { name, keys })
}

/// Teilt eine Key-Liste an Kommas, respektiert dabei Kommas innerhalb von '...'-Strings
fn split_keys(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_string = false;

    for c in raw.chars() {
        match c {
            '\'' => {
                in_string = !in_string;
                current.push(c);
            }
            ',' if !in_string => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn parse_key(raw: &str, segment: &str) -> Result<KeyValue, PathParseError> {
    let caps = KEY_RE.captures(raw).ok_or_else(|| PathParseError {
        segment: segment.to_string(),
        message: format!("ungültiger Schlüssel: '{raw}'"),
    })?;

    let name = caps.name("key").map(|m| m.as_str().to_string());

    let value = if let Some(m) = caps.name("string") {
        KeyLiteral::String(m.as_str().replace("''", "'"))
    } else if let Some(m) = caps.name("guid") {
        KeyLiteral::Guid(m.as_str().to_string())
    } else if let Some(m) = caps.name("bool") {
        KeyLiteral::Bool(m.as_str() == "true")
    } else if let Some(m) = caps.name("number") {
        KeyLiteral::Number(m.as_str().parse().expect("Regex garantiert gültige Zahl"))
    } else {
        unreachable!("eine der Alternativen muss matchen")
    };

    Ok(KeyValue { name, value })
}
