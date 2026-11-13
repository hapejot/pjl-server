use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result, bail};
use quick_xml::{
    Reader, XmlVersion,
    escape::resolve_predefined_entity,
    events::{BytesStart, Event},
};
use serde_json::{Map, Value};

struct Element {
    name: String,
    attributes: Map<String, Value>,
    children: Vec<Element>,
    text: String,
}

impl Element {
    fn start(event: &BytesStart<'_>, reader: &Reader<&[u8]>) -> Result<Self> {
        let name = reader.decoder().decode(event.name().as_ref())?.into_owned();
        let mut attributes = Map::new();
        for attribute in event.attributes() {
            let attribute = attribute.context("XML-Attribut lesen")?;
            let key = reader.decoder().decode(attribute.key.as_ref())?;
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            attributes.insert(
                format!("@{key}"),
                Value::String(
                    attribute
                        .decoded_and_normalized_value(XmlVersion::Explicit1_0, reader.decoder())?
                        .into_owned(),
                ),
            );
        }
        Ok(Self {
            name,
            attributes,
            children: Vec::new(),
            text: String::new(),
        })
    }

    fn find_structure(
        &self,
        path: &mut Vec<String>,
        lists: &mut HashSet<Vec<String>>,
        objects: &mut HashSet<Vec<String>>,
    ) {
        if !self.attributes.is_empty() || !self.children.is_empty() {
            objects.insert(path.clone());
        }
        let mut counts = BTreeMap::new();
        for child in &self.children {
            *counts.entry(&child.name).or_insert(0) += 1;
        }
        for (name, count) in counts {
            if count > 1 {
                path.push(name.clone());
                lists.insert(path.clone());
                path.pop();
            }
        }
        for child in &self.children {
            path.push(child.name.clone());
            child.find_structure(path, lists, objects);
            path.pop();
        }
    }

    fn into_value(
        self,
        path: &mut Vec<String>,
        lists: &HashSet<Vec<String>>,
        objects: &HashSet<Vec<String>>,
    ) -> Value {
        if !objects.contains(path) {
            return Value::String(self.text);
        }
        let mut fields = self.attributes;
        let mut children: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for child in self.children {
            let name = child.name.clone();
            path.push(name.clone());
            let value = child.into_value(path, lists, objects);
            path.pop();
            children.entry(name).or_default().push(value);
        }
        for (name, mut values) in children {
            path.push(name.clone());
            let value = if lists.contains(path) {
                Value::Array(values)
            } else {
                values.remove(0)
            };
            path.pop();
            fields.insert(name, value);
        }
        if !self.text.trim().is_empty() {
            fields.insert("#text".into(), Value::String(self.text));
        }
        Value::Object(fields)
    }
}

fn finish(element: Element, stack: &mut [Element], root: &mut Option<Element>) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(element);
    } else if root.is_none() {
        *root = Some(element);
    } else {
        bail!("Mehr als ein XML-Wurzelelement");
    }
    Ok(())
}

fn append_text(text: &str, stack: &mut [Element]) -> Result<()> {
    if let Some(element) = stack.last_mut() {
        element.text.push_str(text);
    } else if !text.trim().is_empty() {
        bail!("Text ausserhalb des XML-Wurzelelements");
    }
    Ok(())
}

/// Map XML to the existing JSON inference input without guessing scalar types.
/// Repetition is detected across all occurrences of the same element path.
pub fn parse_xml(input: &str) -> Result<Value> {
    let mut reader = Reader::from_str(input.trim_start_matches('\u{feff}'));
    reader.config_mut().check_comments = true;
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    let mut declaration_seen = false;
    loop {
        match reader.read_event().context("XML lesen")? {
            Event::Start(event) => stack.push(Element::start(&event, &reader)?),
            Event::Empty(event) => finish(Element::start(&event, &reader)?, &mut stack, &mut root)?,
            Event::End(_) => {
                let element = stack.pop().context("XML-Endelement ohne Anfang")?;
                finish(element, &mut stack, &mut root)?;
            }
            Event::Text(event) => append_text(&event.xml10_content()?, &mut stack)?,
            Event::CData(event) => {
                if stack.is_empty() {
                    bail!("CDATA ausserhalb des XML-Wurzelelements");
                }
                append_text(&event.xml10_content()?, &mut stack)?;
            }
            Event::GeneralRef(event) => {
                if stack.is_empty() {
                    bail!("XML-Entity ausserhalb des Wurzelelements");
                }
                if let Some(character) = event.resolve_char_ref()? {
                    append_text(&character.to_string(), &mut stack)?;
                } else {
                    let name = event.decode()?;
                    let text = resolve_predefined_entity(&name)
                        .with_context(|| format!("Unbekannte XML-Entity: &{name};"))?;
                    append_text(text, &mut stack)?;
                }
            }
            Event::Decl(declaration) => {
                if declaration_seen || root.is_some() || !stack.is_empty() {
                    bail!("XML-Deklaration nur einmal vor dem Wurzelelement erlaubt");
                }
                declaration_seen = true;
                if declaration.version()?.as_ref() != b"1.0" {
                    bail!("Nur XML 1.0 wird unterstuetzt");
                }
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding?;
                    if !encoding.eq_ignore_ascii_case(b"utf-8") {
                        bail!("XML-Eingabe muss UTF-8 verwenden");
                    }
                }
            }
            Event::DocType(_) => bail!("XML-DTDs werden nicht unterstuetzt"),
            Event::Comment(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        bail!("XML enthaelt nicht geschlossene Elemente");
    }
    let root = root.context("XML-Wurzelelement fehlt")?;
    let mut lists = HashSet::new();
    let mut objects = HashSet::new();
    root.find_structure(&mut Vec::new(), &mut lists, &mut objects);
    Ok(root.into_value(&mut Vec::new(), &lists, &objects))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_attributes_text_repetition_and_preserves_string_values() {
        let value = parse_xml(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <orders>
                <order code="001"><active>true</active><tag>A</tag><empty/></order>
                <order code="002"><active>false</active><tag>B</tag><tag>C</tag></order>
            </orders>"#,
        )
        .unwrap();
        assert_eq!(
            value,
            json!({"order": [
                {"@code": "001", "active": "true", "tag": ["A"], "empty": ""},
                {"@code": "002", "active": "false", "tag": ["B", "C"]}
            ]})
        );
        let generated = crate::generate(&value, "Orders", "sales").unwrap();
        let doc = serde_json::to_value(generated.ddic).unwrap();
        let entities = doc["entities"].as_array().unwrap();
        assert!(entities.iter().any(|e| e["name"] == "Orders_order_tag"));
        let order = entities
            .iter()
            .find(|e| e["name"] == "Orders_order")
            .unwrap();
        let active = order["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["Atom"]["name"] == "active")
            .unwrap();
        assert_eq!(active["Atom"]["edm_type"], "Edm.String");
    }

    #[test]
    fn decodes_entities_cdata_and_keeps_qualified_names() {
        let value = parse_xml(
            r#"<n:root xmlns:n="urn:orders" xmlns:m="urn:other" n:id="A&amp;B">
            Hello &lt; &#65; <![CDATA[<&>]]><n:name>X</n:name><m:name>Y</m:name>!
        </n:root>"#,
        )
        .unwrap();
        assert_eq!(value["@n:id"], "A&B");
        assert_eq!(value["n:name"], "X");
        assert_eq!(value["m:name"], "Y");
        assert!(value["#text"].as_str().unwrap().contains("Hello < A <&>"));
        assert!(value.get("@xmlns:n").is_none());
        assert_eq!(parse_xml("<root>001</root>").unwrap(), "001");
        assert_eq!(parse_xml("<root/>").unwrap(), "");
        assert_eq!(
            parse_xml("<root attr=\"A\r\nB&#10;C\">A\r\nB</root>").unwrap(),
            json!({"@attr": "A B\nC", "#text": "A\nB"})
        );
    }

    #[test]
    fn keeps_optional_attributes_and_children_as_one_object_shape() {
        let value =
            parse_xml("<root><item code='01'>A</item><item>B</item><item/></root>").unwrap();
        assert_eq!(
            value,
            json!({"item": [
                {"@code": "01", "#text": "A"}, {"#text": "B"}, {}
            ]})
        );
        let generated = crate::generate(&value, "Root", "imported").unwrap();
        assert!(!generated.warnings.iter().any(|w| w.contains("JSON-Text")));
    }

    #[test]
    fn rejects_invalid_documents_unsupported_encodings_and_dtds() {
        for input in [
            "",
            "<a>",
            "<a></b>",
            "<a/><b/>",
            "text<a/>",
            "<a/>text",
            "&#32;<a/>",
            "<a duplicate='1' duplicate='2'/>",
            "<a>&missing;</a>",
            "<!DOCTYPE a [<!ENTITY x 'test'>]><a>&x;</a>",
            "<?xml version='1.0' encoding='ISO-8859-1'?><a/>",
            "<?xml version='1.1'?><a/>",
        ] {
            assert!(parse_xml(input).is_err(), "accepted invalid input: {input}");
        }
    }
}
