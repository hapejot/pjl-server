//! Auswertung von `$metadata` (EDMX): Entitaetstypen, Sets, Navigationen und
//! die Reihenfolge, in der Sets angelegt werden koennen.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Navigation {
    pub target: String,
    pub to_one: bool,
    /// (Fremdschluesselproperty, referenzierte Property)
    pub refs: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct EntityType {
    pub name: String,
    pub keys: Vec<String>,
    pub properties: Vec<String>,
    pub navigations: BTreeMap<String, Navigation>,
}

impl EntityType {
    pub fn is_draft(&self) -> bool {
        self.properties.iter().any(|p| p == "IsActiveEntity")
    }

    /// Schluesselfelder ohne das Entwurfs-Flag `IsActiveEntity`.
    pub fn business_keys(&self) -> Vec<&str> {
        self.keys
            .iter()
            .map(String::as_str)
            .filter(|k| *k != "IsActiveEntity")
            .collect()
    }

    /// OData-Schluesselausdruck fuer einen Satz, z. B. `'planned'`, `<guid>` oder
    /// `ID=<guid>,IsActiveEntity=true` (Entwurfs-Sets).
    pub fn key_expr(&self, record: &Value, active: bool) -> Result<String> {
        let keys = self.business_keys();
        if keys == ["ID"] {
            let id = record
                .get("ID")
                .and_then(Value::as_str)
                .context("ID fehlt")?;
            return Ok(if self.is_draft() {
                format!(
                    "ID={id},IsActiveEntity={}",
                    if active { "true" } else { "false" }
                )
            } else {
                id.to_string()
            });
        }
        if let [key] = keys.as_slice() {
            return match record.get(*key) {
                Some(Value::String(s)) => Ok(format!("'{}'", s.replace('\'', "''"))),
                Some(Value::Bool(b)) => Ok(b.to_string()),
                Some(Value::Number(n)) => Ok(n.to_string()),
                _ => bail!("{}: Schluesselfeld '{key}' fehlt", self.name),
            };
        }
        bail!(
            "{}: zusammengesetzter Schluessel {keys:?} wird nicht unterstuetzt",
            self.name
        )
    }
}

#[derive(Debug, Default)]
pub struct Model {
    pub types: BTreeMap<String, EntityType>,
    /// Setname -> Typname
    pub sets: BTreeMap<String, String>,
}

impl Model {
    pub fn parse(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        let mut model = Model::default();
        let mut current: Option<EntityType> = None;
        let mut in_key = false;
        let mut last_nav: Option<String> = None;
        loop {
            let event = reader.read_event().context("ungueltiges $metadata-XML")?;
            match event {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) => {
                    handle_start(&e, &mut model, &mut current, &mut in_key, &mut last_nav)?;
                }
                Event::End(e) => match e.local_name().as_ref() {
                    "EntityType" => {
                        if let Some(t) = current.take() {
                            model.types.insert(t.name.clone(), t);
                        }
                    }
                    "Key" => in_key = false,
                    _ => {}
                },
                _ => {}
            }
        }
        for t in model.types.values() {
            for nav in t.navigations.values() {
                if !model.types.contains_key(&nav.target) {
                    bail!(
                        "{}: Navigationsziel '{}' fehlt in $metadata",
                        t.name,
                        nav.target
                    );
                }
            }
        }
        Ok(model)
    }

    pub fn set_type(&self, set: &str) -> Option<&EntityType> {
        self.sets.get(set).and_then(|t| self.types.get(t))
    }

    /// Setname des Typs, falls vorhanden.
    pub fn set_for_type(&self, type_name: &str) -> Option<&str> {
        self.sets
            .iter()
            .find(|(_, t)| t.as_str() == type_name)
            .map(|(s, _)| s.as_str())
    }

    /// Reihenfolge: ein Set kommt nach allen Sets, auf die es per Fremdschluessel zeigt.
    /// Selbstreferenzen werden ignoriert, Zyklen sind ein Fehler.
    pub fn order(&self, sets: &[String]) -> Result<Vec<String>> {
        let mut remaining: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for s in sets {
            let t = self
                .set_type(s)
                .with_context(|| format!("Set {s} ohne Typ"))?;
            let deps = t
                .navigations
                .values()
                .filter(|n| n.to_one && !n.refs.is_empty())
                .filter_map(|n| self.set_for_type(&n.target).map(str::to_string))
                .filter(|d| d != s && sets.contains(d))
                .collect();
            remaining.insert(s.clone(), deps);
        }
        let mut ordered = Vec::new();
        while !remaining.is_empty() {
            let ready: Vec<String> = remaining
                .iter()
                .filter(|(_, d)| d.is_empty())
                .map(|(s, _)| s.clone())
                .collect();
            if ready.is_empty() {
                bail!(
                    "zyklische Abhaengigkeit zwischen Sets: {:?}",
                    remaining.keys().collect::<Vec<_>>()
                );
            }
            for s in &ready {
                remaining.remove(s);
                ordered.push(s.clone());
            }
            for d in remaining.values_mut() {
                d.retain(|x| !ready.contains(x));
            }
        }
        Ok(ordered)
    }
}

fn handle_start(
    e: &BytesStart,
    model: &mut Model,
    current: &mut Option<EntityType>,
    in_key: &mut bool,
    last_nav: &mut Option<String>,
) -> Result<()> {
    match e.local_name().as_ref() {
        "EntityType" => {
            *current = Some(EntityType {
                name: attr(e, "Name")?.unwrap_or_default(),
                ..Default::default()
            });
        }
        "Key" => *in_key = true,
        "PropertyRef" if *in_key => {
            if let (Some(t), Some(name)) = (current.as_mut(), attr(e, "Name")?) {
                t.keys.push(name);
            }
        }
        "Property" => {
            if let (Some(t), Some(name)) = (current.as_mut(), attr(e, "Name")?) {
                t.properties.push(name);
            }
        }
        "NavigationProperty" => {
            if let (Some(t), Some(name), Some(typ)) =
                (current.as_mut(), attr(e, "Name")?, attr(e, "Type")?)
            {
                let collection = typ.starts_with("Collection(");
                let target = strip_type(&typ);
                *last_nav = Some(name.clone());
                t.navigations.insert(
                    name,
                    Navigation {
                        target,
                        to_one: !collection,
                        refs: Vec::new(),
                    },
                );
            }
        }
        "ReferentialConstraint" => {
            // Gehoert zur zuletzt gelesenen Navigation desselben Typs.
            if let (Some(t), Some(prop), Some(referenced)) = (
                current.as_mut(),
                attr(e, "Property")?,
                attr(e, "ReferencedProperty")?,
            ) {
                if let Some(nav) = last_nav.as_ref().and_then(|n| t.navigations.get_mut(n)) {
                    nav.refs.push((prop, referenced));
                }
            }
        }
        "EntitySet" => {
            if let (Some(name), Some(typ)) = (attr(e, "Name")?, attr(e, "EntityType")?) {
                model.sets.insert(name, strip_type(&typ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn attr(e: &BytesStart, name: &str) -> Result<Option<String>> {
    for a in e.attributes() {
        let a = a?;
        if a.key.local_name().as_ref() == name {
            return Ok(Some(
                a.normalized_value(XmlVersion::Implicit1_0)?.into_owned(),
            ));
        }
    }
    Ok(None)
}

/// `Service.Project` -> `Project`, `Collection(Service.Task)` -> `Task`.
fn strip_type(name: &str) -> String {
    let inner = name.trim_start_matches("Collection(").trim_end_matches(')');
    inner.rsplit('.').next().unwrap_or(inner).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_namespaced_metadata_with_unicode_and_escaped_attributes() {
        let model = Model::parse(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx">
                <edmx:DataServices>
                    <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Service">
                        <EntityType Name="Category">
                            <Key><PropertyRef Name="ID"/></Key>
                            <Property Name="ID" Type="Edm.Guid"/>
                        </EntityType>
                        <EntityType Name="Product">
                            <Key>
                                <PropertyRef Name="ID"/>
                                <PropertyRef Name="IsActiveEntity"/>
                            </Key>
                            <Property Name="ID" Type="Edm.Guid"/>
                            <Property Name="IsActiveEntity" Type="Edm.Boolean"/>
                            <Property Name="Catégorie_ID" Type="Edm.Guid"/>
                            <NavigationProperty Name="Cat&#xE9;gory" Type="Service.Category">
                                <ReferentialConstraint Property="Catégorie_ID" ReferencedProperty="ID"/>
                            </NavigationProperty>
                        </EntityType>
                        <EntityContainer Name="Container">
                            <EntitySet Name="Cat&#xE9;gories" EntityType="Service.Category"/>
                            <EntitySet Name="Products" EntityType="Service.Product"/>
                        </EntityContainer>
                    </Schema>
                </edmx:DataServices>
            </edmx:Edmx>"#,
        )
        .unwrap();

        let product = model.set_type("Products").unwrap();
        assert_eq!(product.business_keys(), ["ID"]);
        assert!(product.is_draft());
        assert!(product.properties.iter().any(|p| p == "Catégorie_ID"));
        let navigation = &product.navigations["Catégory"];
        assert_eq!(navigation.target, "Category");
        assert!(navigation.to_one);
        assert_eq!(navigation.refs, [("Catégorie_ID".into(), "ID".into())]);
        assert_eq!(
            model
                .order(&["Products".into(), "Catégories".into()])
                .unwrap(),
            ["Catégories", "Products"]
        );
    }
}
