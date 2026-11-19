use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Annotation {
    #[serde(rename = "@Term")]
    pub term: String,

    #[serde(rename = "@Qualifier", skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,

    #[serde(rename = "@String", skip_serializing_if = "Option::is_none")]
    pub string: Option<String>,

    #[serde(rename = "@Bool", skip_serializing_if = "Option::is_none")]
    pub bool_: Option<bool>,

    #[serde(rename = "@Path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(rename = "@EnumMember", skip_serializing_if = "Option::is_none")]
    pub enum_member: Option<String>,

    #[serde(rename = "Collection", skip_serializing_if = "Option::is_none")]
    pub collection: Option<Collection>,

    #[serde(rename = "Record", skip_serializing_if = "Option::is_none")]
    pub record: Option<Record>,

    // z.B. Common.Text hat eine verschachtelte <Annotation> als Kind-Element
    #[serde(rename = "Annotation", default, skip_serializing_if = "Vec::is_empty")]
    pub nested_annotations: Vec<Annotation>,
}

impl Annotation {
    pub fn new(term: impl Into<String>) -> Self {
        Self {
            term: term.into(),
            qualifier: None,
            string: None,
            bool_: None,
            path: None,
            enum_member: None,
            collection: None,
            record: None,
            nested_annotations: vec![],
        }
    }

    pub fn string(mut self, value: impl Into<String>) -> Self {
        self.string = Some(value.into());
        self
    }

    pub fn boolean(mut self, value: bool) -> Self {
        self.bool_ = Some(value);
        self
    }

    pub fn record(mut self, record: Record) -> Self {
        self.record = Some(record);
        self
    }
    pub fn collection(mut self, collection: Collection) -> Self {
        self.collection = Some(collection);
        self
    }
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn enum_member(mut self, arg: impl Into<String>) -> Self {
        self.enum_member = Some(arg.into());
        self
    }

    pub(crate) fn annotation(mut self, enum_member: Annotation) -> Self {
        self.nested_annotations.push(enum_member);
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Collection {
    #[serde(
        rename = "PropertyPath",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub property_paths: Vec<String>,

    #[serde(
        rename = "NavigationPropertyPath",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub navigation_property_paths: Vec<String>,

    #[serde(
        rename = "AnnotationPath",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub annotation_paths: Vec<String>,

    #[serde(rename = "Record", default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<Record>,
}

impl Collection {
    pub fn new() -> Self {
        Self {
            property_paths: Vec::new(),
            navigation_property_paths: Vec::new(),
            annotation_paths: Vec::new(),
            records: Vec::new(),
        }
    }

    pub fn add_annotation_path(mut self, path: impl Into<String>) -> Self {
        self.annotation_paths.push(path.into());
        self
    }

    pub fn add_property_path(mut self, path: impl Into<String>) -> Self {
        self.property_paths.push(path.into());
        self
    }

    pub fn add_navigation_property_path(mut self, path: impl Into<String>) -> Self {
        self.navigation_property_paths.push(path.into());
        self
    }

    pub fn add_record(mut self, record: Record) -> Self {
        self.records.push(record);
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Record {
    #[serde(rename = "@Type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    #[serde(rename = "PropertyValue", default)]
    pub property_values: Vec<PropertyValue>,

    /// Annotationen am Record selbst (z. B. `UI.Hidden` an einem `UI.DataField`).
    #[serde(rename = "Annotation", default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

impl Record {
    pub fn new(type_: impl Into<String>) -> Self {
        Self {
            type_: Some(type_.into()),
            property_values: Vec::new(),
            annotations: Vec::new(),
        }
    }
    pub fn add(mut self, property_value: PropertyValue) -> Self {
        self.property_values.push(property_value);
        self
    }
    pub fn annotation(mut self, annotation: Annotation) -> Self {
        self.annotations.push(annotation);
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PropertyValue {
    #[serde(rename = "@Property")]
    pub property: String,

    #[serde(rename = "@Path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(rename = "@String", skip_serializing_if = "Option::is_none")]
    pub string: Option<String>,

    #[serde(rename = "@Bool", skip_serializing_if = "Option::is_none")]
    pub bool_: Option<bool>,

    #[serde(rename = "@Constant", skip_serializing_if = "Option::is_none")]
    pub constant: Option<String>,

    #[serde(rename = "@PropertyPath", skip_serializing_if = "Option::is_none")]
    pub property_path: Option<String>,

    #[serde(rename = "@AnnotationPath", skip_serializing_if = "Option::is_none")]
    pub annotation_path: Option<String>,

    #[serde(rename = "Collection", skip_serializing_if = "Option::is_none")]
    pub collection: Option<Collection>,

    #[serde(rename = "Record", skip_serializing_if = "Option::is_none")]
    pub record: Option<Record>,
}

impl PropertyValue {
    pub fn new(property: impl Into<String>) -> Self {
        Self {
            property: property.into(),
            path: None,
            string: None,
            bool_: None,
            constant: None,
            property_path: None,
            annotation_path: None,
            collection: None,
            record: None,
        }
    }

    pub fn string(mut self, value: impl Into<String>) -> Self {
        self.string = Some(value.into());
        self
    }
    pub fn boolean(mut self, value: impl Into<bool>) -> Self {
        self.bool_ = Some(value.into());
        self
    }

    pub fn constant(mut self, value: impl Into<String>) -> Self {
        self.constant = Some(value.into());
        self
    }

    pub fn record(mut self, record: Record) -> Self {
        self.record = Some(record);
        self
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn property_path(mut self, path: impl Into<String>) -> Self {
        self.property_path = Some(path.into());
        self
    }

    pub fn collection(mut self, collection: Collection) -> Self {
        self.collection = Some(collection);
        self
    }

    pub fn annotation_path(mut self, annotation_path: impl Into<String>) -> Self {
        self.annotation_path = Some(annotation_path.into());
        self
    }
}
