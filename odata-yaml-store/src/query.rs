use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct ODataQuery {
    pub filter: Option<FilterExpr>,
    pub orderby: Vec<OrderByItem>,
    pub top: Option<u32>,
    pub skip: Option<u32>,
    /// `None` = alle Felder (kein `$select` angegeben)
    pub select: Option<Vec<String>>,
    pub expand: Vec<ExpandItem>,
    /// `$count=true` (V4) bzw. `$inlinecount=allpages` (V2)
    pub count: bool,
    pub search: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OrderByItem {
    pub property: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct ExpandItem {
    pub navigation_property: String,
    /// Verschachtelte Query auf der expandierten Collection (rekursiv).
    pub nested_query: Box<ODataQuery>,
}

#[derive(Debug, Clone)]
pub enum FilterExpr {
    Compare {
        left: FilterOperand,
        op: CompareOp,
        right: FilterOperand,
    },
    And(Box<FilterExpr>, Box<FilterExpr>),
    Or(Box<FilterExpr>, Box<FilterExpr>),
    Not(Box<FilterExpr>),
    /// z.B. contains(Name,'foo'), startswith(...), substringof(...) (V2)
    Function {
        name: String,
        args: Vec<FilterOperand>,
    },
    /// $filter=Items/any(i:i/Qty gt 10)
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

#[derive(Debug, Clone)]
pub enum FilterOperand {
    Property(String),
    Literal(Value),
    Function { name: String, args: Vec<FilterOperand> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Kleine Bau-Helfer, damit man in Tests/Beispielen nicht staendig
/// die vollen Enum-Pfade ausschreiben muss.
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
