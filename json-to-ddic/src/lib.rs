use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, bail};
use serde_json::Value;
use spec::{
    EntitySpec, FieldSpec, Relationship, data_dictionary::DataDictionary, package::Package,
};

pub mod xml;

pub struct Generated {
    pub ddic: DataDictionary,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Shape {
    Unknown,
    Boolean,
    Int32,
    Int64,
    Decimal { integer_digits: u32, scale: u32 },
    String,
    Object(BTreeMap<String, Shape>),
    Array(Box<Shape>),
    JsonText,
}

impl Shape {
    fn infer(value: &Value) -> Self {
        match value {
            Value::Null => Self::Unknown,
            Value::Bool(_) => Self::Boolean,
            Value::Number(n) => {
                if let Some(n) = n.as_i64() {
                    if i32::try_from(n).is_ok() {
                        Self::Int32
                    } else {
                        Self::Int64
                    }
                } else {
                    Self::decimal_shape(&n.to_string())
                }
            }
            Value::String(_) => Self::String,
            Value::Object(fields) => Self::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::infer(v)))
                    .collect(),
            ),
            Value::Array(values) => Self::Array(Box::new(
                values.iter().fold(Self::Unknown, |shape, value| {
                    shape.merge(Self::infer(value))
                }),
            )),
        }
    }

    fn merge(self, other: Self) -> Self {
        use Shape::*;
        match (self, other) {
            (Unknown, b) => b,
            (a, Unknown) => a,
            (a, b) if a == b => a,
            (Int32, Int64) | (Int64, Int32) => Int64,
            (
                Decimal {
                    integer_digits: a,
                    scale: sa,
                },
                Decimal {
                    integer_digits: b,
                    scale: sb,
                },
            ) => Decimal {
                integer_digits: a.max(b),
                scale: sa.max(sb),
            },
            (d @ Decimal { .. }, Int32) | (Int32, d @ Decimal { .. }) => d.merge(Decimal {
                integer_digits: 10,
                scale: 0,
            }),
            (d @ Decimal { .. }, Int64) | (Int64, d @ Decimal { .. }) => d.merge(Decimal {
                integer_digits: 19,
                scale: 0,
            }),
            (Object(mut a), Object(b)) => {
                for (key, shape) in b {
                    let previous = a.remove(&key).unwrap_or(Unknown);
                    a.insert(key, previous.merge(shape));
                }
                Object(a)
            }
            (Array(a), Array(b)) => Array(Box::new(a.merge(*b))),
            _ => JsonText,
        }
    }

    fn decimal_shape(number: &str) -> Shape {
        let number = number.trim_start_matches('-');
        let (mantissa, exponent) = number.split_once(['e', 'E']).unwrap_or((number, "0"));
        let Ok(exponent) = exponent.parse::<i32>() else {
            return Shape::JsonText;
        };
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let integer_digits = (whole.len() as i64 + i64::from(exponent)).max(0);
        let scale = (fraction.len() as i64 - i64::from(exponent)).max(0);
        match (u32::try_from(integer_digits), u32::try_from(scale)) {
            (Ok(integer_digits), Ok(scale))
                if integer_digits.checked_add(scale.max(3)).is_some() =>
            {
                Shape::Decimal {
                    integer_digits,
                    scale,
                }
            }
            _ => Shape::JsonText,
        }
    }

    fn fields(self) -> BTreeMap<String, Self> {
        match self {
            Self::Object(fields) => fields,
            scalar => BTreeMap::from([("value".into(), scalar)]),
        }
    }
}

fn identifier(value: &str) -> String {
    let mut name: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert(0, '_');
    }
    name
}

fn unique(base: &str, used: &mut HashSet<String>) -> String {
    let mut name = base.to_owned();
    let mut suffix = 2;
    while !used.insert(name.clone()) {
        name = format!("{base}_{suffix}");
        suffix += 1;
    }
    name
}

struct Builder {
    ddic: DataDictionary,
    package: String,
    names: HashSet<String>,
    warnings: Vec<String>,
}

impl Builder {
    fn entity(
        &mut self,
        name: &str,
        fields: BTreeMap<String, Shape>,
        path: &str,
        collection_item: bool,
        reverse: Option<&str>,
    ) {
        let mut entity = EntitySpec::new(name, &self.package, name)
            .without_audit()
            .without_drafts();
        let mut used = HashSet::from(["ID".to_owned()]);
        if let Some(reverse) = reverse {
            used.insert(reverse.into());
            used.insert(format!("{reverse}_ID"));
        }
        if collection_item {
            used.insert("_index".into());
            entity.add_field(FieldSpec::atom("_index", "Index", "Edm.Int64"));
        }
        // Allocate source fields before generated navigation/FK names.
        let fields: Vec<_> = fields
            .into_iter()
            .map(|(source, shape)| {
                let field = unique(&identifier(&source), &mut used);
                let field_path = format!("{path}[{}]", serde_json::to_string(&source).unwrap());
                if field != source {
                    self.warnings
                        .push(format!("{field_path}: Feld heisst im DDIC {field}"));
                }
                (source, field, field_path, shape)
            })
            .collect();
        for (label, field, field_path, shape) in fields {
            match shape {
                Shape::Object(children) => {
                    let child = unique(&format!("{name}_{field}"), &mut self.names);
                    let fk = format!("{field}_ID");
                    if used.contains(&fk) {
                        // Keep the source scalar and rename only the navigation.
                        let nav = self.navigation_name(&field, &mut used);
                        self.warnings.push(format!(
                            "{field_path}: Navigation heisst wegen FK-Kollision {nav}"
                        ));
                        self.object(&child, children, &field_path, name, &nav, &label);
                    } else {
                        used.insert(fk);
                        self.object(&child, children, &field_path, name, &field, &label);
                    }
                }
                Shape::Array(item) if *item != Shape::Unknown && *item != Shape::JsonText => {
                    let child = unique(&format!("{name}_{field}"), &mut self.names);
                    self.entity(
                        &child,
                        item.fields(),
                        &format!("{field_path}[]"),
                        true,
                        Some("_parent"),
                    );
                    let mut rel = Relationship::new(format!("{child}_items"), &self.package)
                        .with_one(name, &field)
                        .with_many(&child, "_parent")
                        .owned();
                    rel.set_one_label(Some(label));
                    self.ddic.add_relationship(rel);
                }
                Shape::Decimal {
                    integer_digits,
                    scale,
                } => {
                    let scale = scale.max(3);
                    entity.add_field(FieldSpec::decimal(
                        &field,
                        &label,
                        (integer_digits + scale).max(38),
                        scale,
                    ));
                }
                scalar => {
                    let edm = match scalar {
                        Shape::Boolean => "Edm.Boolean",
                        Shape::Int32 => "Edm.Int32",
                        Shape::Int64 => "Edm.Int64",
                        Shape::String => "Edm.String",
                        _ => {
                            self.warnings.push(format!(
                                "{field_path}: Typ nicht eindeutig; Edm.String fuer JSON-Text verwenden"
                            ));
                            "Edm.String"
                        }
                    };
                    entity.add_field(FieldSpec::atom(&field, &label, edm));
                }
            }
        }
        self.ddic.add_entity(entity);
    }

    fn navigation_name(&self, field: &str, used: &mut HashSet<String>) -> String {
        let base = format!("{field}_ref");
        loop {
            let nav = unique(&base, used);
            if used.insert(format!("{nav}_ID")) {
                return nav;
            }
        }
    }

    fn object(
        &mut self,
        child: &str,
        fields: BTreeMap<String, Shape>,
        path: &str,
        parent: &str,
        nav: &str,
        label: &str,
    ) {
        self.entity(child, fields, path, false, Some("_parents"));
        let mut rel = Relationship::new(format!("{child}_reference"), &self.package)
            .with_one(child, "_parents")
            .with_many(parent, nav);
        rel.set_many_label(Some(label.into()));
        self.ddic.add_relationship(rel);
    }
}

/// Infer a schema, not data: nulls contribute no type evidence; all array elements
/// contribute to the union of fields and compatible numeric types.
pub fn generate(value: &Value, entity: &str, package: &str) -> Result<Generated> {
    for (label, name) in [("entity", entity), ("package", package)] {
        if name.is_empty() || identifier(name) != name {
            bail!("{label}: '{name}' ist kein ASCII-Bezeichner");
        }
    }
    let mut builder = Builder {
        ddic: DataDictionary::empty(),
        package: package.into(),
        names: HashSet::from([entity.into()]),
        warnings: Vec::new(),
    };
    builder.ddic.add_package(Package::new(package));
    let shape = Shape::infer(value);
    let (shape, collection) = match shape {
        Shape::Array(item) => (*item, true),
        other => (other, false),
    };
    builder.entity(entity, shape.fields(), "$", collection, None);
    builder
        .ddic
        .validate_primary_keys()
        .map_err(anyhow::Error::msg)?;
    builder
        .ddic
        .validate_list_views()
        .map_err(anyhow::Error::msg)?;
    let mut active = builder.ddic.clone();
    active.activate();
    spec_schema::schema_from_ddic(&active).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    Ok(Generated {
        ddic: builder.ddic,
        warnings: builder.warnings,
    })
}

#[cfg(test)]
mod tests;
