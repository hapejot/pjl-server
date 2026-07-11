use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ODataQuery {
    #[serde(rename = "$filter")]
    pub filter: Option<FilterExpr>,
    #[serde(rename = "$orderby")]
    pub orderby: Vec<OrderByItem>,
    #[serde(rename = "$top")]
    pub top: Option<u32>,
    #[serde(rename = "$skip")]
    pub skip: Option<u32>,
    #[serde(rename = "$select")]
    pub select: Option<Vec<String>>,
    #[serde(rename = "$expand")]
    pub expand: Vec<ExpandItem>,
    #[serde(rename = "$count")]
    pub count: bool,
    #[serde(rename = "$search")]
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderByItem {
    pub property: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpandItem {
    pub navigation_property: String,
    pub nested_query: Box<ODataQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterExpr {
    Compare {
        left: FilterOperand,
        op: CompareOp,
        right: FilterOperand,
    },
    And(Box<FilterExpr>, Box<FilterExpr>),
    Or(Box<FilterExpr>, Box<FilterExpr>),
    Not(Box<FilterExpr>),
    Function {
        name: String,
        args: Vec<FilterOperand>,
    },
    Any {
        collection_property: String,
        lambda_var: String,
        predicate: Box<FilterExpr>,
    },
    All {
        collection_property: String,
        lambda_var: String,
        predicate: Box<FilterExpr>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterOperand {
    Property(String),
    Literal(Value),
    Function {
        name: String,
        args: Vec<FilterOperand>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

impl FilterExpr {
    pub fn eq(prop: &str, value: impl Into<Value>) -> Self {
        FilterExpr::Compare {
            left: FilterOperand::Property(prop.to_string()),
            op: CompareOp::Eq,
            right: FilterOperand::Literal(value.into()),
        }
    }

    pub fn gt(prop: &str, value: impl Into<Value>) -> Self {
        FilterExpr::Compare {
            left: FilterOperand::Property(prop.to_string()),
            op: CompareOp::Gt,
            right: FilterOperand::Literal(value.into()),
        }
    }

    pub fn and(self, other: FilterExpr) -> Self {
        FilterExpr::And(Box::new(self), Box::new(other))
    }

    pub fn or(self, other: FilterExpr) -> Self {
        FilterExpr::Or(Box::new(self), Box::new(other))
    }
}
