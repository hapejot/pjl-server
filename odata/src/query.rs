use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ODataQuery {
    #[serde(rename = "$filter")]
    pub filter: Option<Expr>,
    #[serde(rename = "$orderby")]
    pub orderby: Vec<OrderByItem>,
    #[serde(rename = "$top")]
    pub top: Option<u32>,
    #[serde(rename = "$skip")]
    pub skip: Option<u32>,
    #[serde(rename = "$select")]
    pub select: Vec<String>,
    #[serde(rename = "$expand")]
    pub expand: Vec<ExpandItem>,
    #[serde(rename = "$count")]
    pub count: bool,
    #[serde(rename = "$search")]
    pub search: Option<String>,
}

impl ODataQuery {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    pub fn order_by(mut self, arg: &str, desc: SortDirection) -> Self {
        self.orderby.push(OrderByItem {
            property: arg.into(),
            direction: desc,
        });
        self
    }

    pub fn filter(mut self, expr: Expr) -> ODataQuery {
        self.filter = Some(expr);
        self
    }

    pub fn expand(mut self, arg: &str) -> ODataQuery {
        self.expand.push(ExpandItem {
            navigation_property: arg.into(),
            nested_query: Default::default(),
        });
        self
    }

    pub fn select(mut self, field: &str) -> Self {
        self.select.push(field.into());
        self
    }
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

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub enum FilterExpr {
//     Compare {
//         left: FilterOperand,
//         op: CompareOp,
//         right: FilterOperand,
//     },
//     And(Box<FilterExpr>, Box<FilterExpr>),
//     Or(Box<FilterExpr>, Box<FilterExpr>),
//     Not(Box<FilterExpr>),
//     Function {
//         name: String,
//         args: Vec<FilterOperand>,
//     },
//     Any {
//         collection_property: String,
//         lambda_var: String,
//         predicate: Box<FilterExpr>,
//     },
//     All {
//         collection_property: String,
//         lambda_var: String,
//         predicate: Box<FilterExpr>,
//     },
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterOperand {
    Property(String),
    Literal(Value),
    Function {
        name: String,
        args: Vec<FilterOperand>,
    },
}

// #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// pub enum CompareOp {
//     Eq,
//     Ne,
//     Gt,
//     Ge,
//     Lt,
//     Le,
// }

// impl FilterExpr {
//     pub fn parse(src: &str) -> Self {
//         Self::Compare {
//             left: FilterOperand::Property("x".into()),
//             op: CompareOp::Eq,
//             right: FilterOperand::Literal(src.into()),
//         }
//     }
//     pub fn eq(prop: &str, value: impl Into<Value>) -> Self {
//         FilterExpr::Compare {
//             left: FilterOperand::Property(prop.to_string()),
//             op: CompareOp::Eq,
//             right: FilterOperand::Literal(value.into()),
//         }
//     }

//     pub fn gt(prop: &str, value: impl Into<Value>) -> Self {
//         FilterExpr::Compare {
//             left: FilterOperand::Property(prop.to_string()),
//             op: CompareOp::Gt,
//             right: FilterOperand::Literal(value.into()),
//         }
//     }

//     pub fn and(self, other: FilterExpr) -> Self {
//         FilterExpr::And(Box::new(self), Box::new(other))
//     }

//     pub fn or(self, other: FilterExpr) -> Self {
//         FilterExpr::Or(Box::new(self), Box::new(other))
//     }
// }
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Identifier(String),
    String(String),
    Number(f64),
    /// ISO-8601-Literal ohne Quotes, z. B. `2026-03-21T23:43:13.863Z` (Edm.DateTimeOffset)
    /// oder `2026-03-21` (Edm.Date). Wird unveraendert als Text weitergereicht.
    DateTime(String),
    Guid(String),
    Bool(bool),
    Null,
    And,
    Or,
    Not,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    LParen,
    RParen,
    Comma,
    Slash,
    Colon,
}

#[derive(Debug)]
pub struct TokenizeError {
    pub position: usize,
    pub snippet: String,
}

static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?P<ws>\s+)",
        r"|(?P<string>'(?:[^']|'')*')",
        r"|(?P<guid>[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})",
        r"|(?P<datetime>\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:\d{2})?)?)",
        r"|(?P<number>-?\d+(?:\.\d+)?)",
        r"|(?P<lparen>\()",
        r"|(?P<rparen>\))",
        r"|(?P<comma>,)",
        r"|(?P<slash>/)",
        r"|(?P<colon>:)",
        r"|(?P<ident>[A-Za-z_][A-Za-z0-9_]*)",
    ))
    .expect("gültige Token-Regex")
});

pub fn tokenize(input: &str) -> Result<Vec<Token>, TokenizeError> {
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < input.len() {
        let Some(caps) = TOKEN_RE.captures(&input[pos..]) else {
            return Err(TokenizeError {
                position: pos,
                snippet: input[pos..].chars().take(10).collect(),
            });
        };

        let whole = caps.get(0).unwrap();

        // Regex muss direkt an aktueller Position matchen, sonst liegt
        // dazwischen ein nicht erkanntes Zeichen.
        if whole.start() != 0 {
            return Err(TokenizeError {
                position: pos,
                snippet: input[pos..pos + whole.start()].to_string(),
            });
        }

        if let Some(m) = caps.name("ws") {
            // Whitespace wird verworfen
            let _ = m;
        } else if let Some(m) = caps.name("string") {
            let raw = m.as_str();
            let content = &raw[1..raw.len() - 1]; // äußere Quotes entfernen
            let unescaped = content.replace("''", "'"); // OData: '' ist escaped '
            tokens.push(Token::String(unescaped));
        } else if let Some(m) = caps.name("guid") {
            tokens.push(Token::Guid(m.as_str().to_string()));
        } else if let Some(m) = caps.name("datetime") {
            tokens.push(Token::DateTime(m.as_str().to_string()));
        } else if let Some(m) = caps.name("number") {
            let n: f64 = m.as_str().parse().expect("Regex garantiert gültige Zahl");
            tokens.push(Token::Number(n));
        } else if caps.name("lparen").is_some() {
            tokens.push(Token::LParen);
        } else if caps.name("rparen").is_some() {
            tokens.push(Token::RParen);
        } else if caps.name("comma").is_some() {
            tokens.push(Token::Comma);
        } else if caps.name("slash").is_some() {
            tokens.push(Token::Slash);
        } else if caps.name("colon").is_some() {
            tokens.push(Token::Colon);
        } else if let Some(m) = caps.name("ident") {
            tokens.push(classify_ident(m.as_str()));
        }

        pos += whole.end();
    }

    Ok(tokens)
}

fn classify_ident(s: &str) -> Token {
    match s.to_ascii_lowercase().as_str() {
        "and" => Token::And,
        "or" => Token::Or,
        "not" => Token::Not,
        "eq" => Token::Eq,
        "ne" => Token::Ne,
        "gt" => Token::Gt,
        "ge" => Token::Ge,
        "lt" => Token::Lt,
        "le" => Token::Le,
        "true" => Token::Bool(true),
        "false" => Token::Bool(false),
        "null" => Token::Null,
        _ => Token::Identifier(s.to_string()),
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Identifier(String),
    Path(Vec<String>), // z.B. Category/Name
    String(String),
    Number(f64),
    DateTime(String),
    Bool(bool),
    Guid(String),
    Null,
    FunctionCall {
        name: String,
        args: Vec<Expr>,
    },
    UnaryNot(Box<Expr>),
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Lambda {
        kind: LambdaKind,
        collection: Vec<String>,
        var: String,
        predicate: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LambdaKind {
    Any,
    All,
}

impl LambdaKind {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "any" => Some(LambdaKind::Any),
            "all" => Some(LambdaKind::All),
            _ => None,
        }
    }
}

impl Expr {
    pub fn equals_guid(guid: impl Into<String>, identifier: impl Into<String>) -> Self {
        Expr::BinaryOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Guid(guid.into())),
            right: Box::new(Expr::Identifier(identifier.into())),
        }
    }

    pub fn equals_value(guid: &Value, identifier: impl Into<String>) -> Self {
        match guid {
            Value::String(s) => Expr::BinaryOp {
                op: BinOp::Eq,
                left: Box::new(Expr::String(s.clone())),
                right: Box::new(Expr::Identifier(identifier.into())),
            },

            Value::Null => Expr::Bool(false),
            Value::Bool(b) => Expr::BinaryOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Bool(*b)),
                right: Box::new(Expr::Identifier(identifier.into())),
            },

            Value::Number(_) => todo!(),
            Value::Array(_) => todo!(),
            Value::Object(_) => todo!(),
        }
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        let tokens = tokenize(input).map_err(|e| {
            format!(
                "Tokenizer-Fehler an Position {}: '{}' von '{}'",
                e.position, e.snippet, input
            )
        })?;
        Parser::new(tokens).parse().map_err(|e| {
            format!(
                "Parse-Fehler bei Token {}: {} von '{}'",
                e.position, e.message, input
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BinOp {
    And,
    Or,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_or()?;
        if self.pos != self.tokens.len() {
            return Err(self.error(&format!(
                "Unerwartetes Token nach Ausdrucksende: {:?}",
                self.peek()
            )));
        }
        Ok(expr)
    }

    // or := and ("or" and)*
    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.check(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // and := not ("and" not)*
    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not()?;
        while self.check(&Token::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // not := "not" not | comparison
    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if self.check(&Token::Not) {
            self.advance();
            let inner = self.parse_not()?;
            return Ok(Expr::UnaryNot(Box::new(inner)));
        }
        self.parse_comparison()
    }

    // comparison := primary (("eq"|"ne"|"gt"|"ge"|"lt"|"le") primary)?
    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_primary()?;

        let op = match self.peek() {
            Some(Token::Eq) => Some(BinOp::Eq),
            Some(Token::Ne) => Some(BinOp::Ne),
            Some(Token::Gt) => Some(BinOp::Gt),
            Some(Token::Ge) => Some(BinOp::Ge),
            Some(Token::Lt) => Some(BinOp::Lt),
            Some(Token::Le) => Some(BinOp::Le),
            _ => None,
        };

        let Some(op) = op else {
            return Ok(left);
        };

        self.advance();
        let right = self.parse_primary()?;

        Ok(Expr::BinaryOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    // primary := "(" or ")" | literal | path | funcCall
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().cloned() {
            Some(Token::LParen) => {
                self.advance();
                let inner = self.parse_or()?;
                self.expect(&Token::RParen)?;
                Ok(inner)
            }
            Some(Token::String(s)) => {
                self.advance();
                Ok(Expr::String(s))
            }
            Some(Token::Number(n)) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Some(Token::DateTime(d)) => {
                self.advance();
                Ok(Expr::DateTime(d))
            }
            Some(Token::Bool(b)) => {
                self.advance();
                Ok(Expr::Bool(b))
            }
            Some(Token::Guid(g)) => {
                self.advance();
                Ok(Expr::Guid(g))
            }
            Some(Token::Null) => {
                self.advance();
                Ok(Expr::Null)
            }
            Some(Token::Identifier(name)) => {
                self.advance();

                // Funktionsaufruf: identifier "(" args ")"
                if self.check(&Token::LParen) {
                    self.advance();
                    let args = self.parse_args()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::FunctionCall { name, args });
                }

                // Pfad-Navigation: identifier ("/" identifier)*
                let mut segments = vec![name];
                while self.check(&Token::Slash) {
                    self.advance();
                    match self.peek().cloned() {
                        Some(Token::Identifier(next)) => {
                            self.advance();
                            if let Some(kind) = LambdaKind::from_name(&next) {
                                if self.check(&Token::LParen) {
                                    return self.parse_lambda(kind, segments);
                                }
                            }
                            segments.push(next);
                        }
                        other => {
                            return Err(self.error(&format!(
                                "Erwartete Identifier nach '/', gefunden: {other:?}"
                            )));
                        }
                    }
                }

                if segments.len() == 1 {
                    Ok(Expr::Identifier(segments.into_iter().next().unwrap()))
                } else {
                    Ok(Expr::Path(segments))
                }
            }
            other => Err(self.error(&format!("Unerwartetes Token: {other:?}"))),
        }
    }

    // lambda := "(" identifier ":" or ")"
    fn parse_lambda(&mut self, kind: LambdaKind, collection: Vec<String>) -> Result<Expr, ParseError> {
        self.expect(&Token::LParen)?;
        let var = match self.peek().cloned() {
            Some(Token::Identifier(v)) => {
                self.advance();
                v
            }
            other => {
                return Err(self.error(&format!(
                    "Erwartete Lambda-Variable, gefunden: {other:?}"
                )));
            }
        };
        self.expect(&Token::Colon)?;
        let predicate = self.parse_or()?;
        self.expect(&Token::RParen)?;
        Ok(Expr::Lambda {
            kind,
            collection,
            var,
            predicate: Box::new(predicate),
        })
    }

    // args := (or ("," or)*)?
    fn parse_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();

        if self.check(&Token::RParen) {
            return Ok(args); // keine Argumente
        }

        args.push(self.parse_or()?);
        while self.check(&Token::Comma) {
            self.advance();
            args.push(self.parse_or()?);
        }

        Ok(args)
    }

    // --- Hilfsfunktionen ---

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn check(&self, expected: &Token) -> bool {
        self.peek() == Some(expected)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        if self.check(expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!(
                "Erwartete {expected:?}, gefunden: {:?}",
                self.peek()
            )))
        }
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError {
            message: message.to_string(),
            position: self.pos,
        }
    }
}

pub fn parse_filter(input: &str) -> Result<Expr, String> {
    Expr::parse(input)
}

#[cfg(test)]
mod datetime_literal_tests {
    use super::*;

    #[test]
    fn datetime_offset_literal_parses_as_one_token() {
        let input = "name eq 'x' and created_at eq 2026-03-21T23:43:13.8630000Z and IsActiveEntity eq true";
        let expr = parse_filter(input).expect("Filter mit Zeitliteral parst");
        let text = format!("{expr:?}");
        assert!(text.contains("DateTime(\"2026-03-21T23:43:13.8630000Z\")"), "{text}");
    }

    #[test]
    fn plain_date_and_negative_numbers_still_work() {
        assert!(parse_filter("due eq 2026-03-21").is_ok());
        assert!(parse_filter("Amount gt -3").is_ok());
    }
}
