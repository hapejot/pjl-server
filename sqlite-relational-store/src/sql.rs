//! Uebersetzung von `$filter` und `$orderby` nach SQL.
//!
//! Zwei Regeln bestimmen das Design:
//!
//! 1. **Nie interpolieren.** Werte gehen ausschliesslich als gebundene
//!    Parameter in die Anweisung.
//! 2. **Nie stillschweigend abweichen.** Was nicht uebersetzbar ist, liefert
//!    [`StoreError`] - ein Filter, der klammheimlich ignoriert wird, gibt
//!    plausible falsche Daten zurueck, und das ist schlimmer als ein Fehler.

use data_store::schema::EntitySet;
use data_store::StoreError;
use odata::query::{BinOp, Expr, ODataQuery, SortDirection};
use rusqlite::types::Value as SqlValue;

use crate::ddl::quote;
use crate::value::json_to_sql_loose;

/// Ergebnis einer Uebersetzung: SQL-Fragment plus zugehoerige Parameter.
pub struct Sql {
    pub text: String,
    pub params: Vec<SqlValue>,
}

fn unsupported(what: impl std::fmt::Display) -> StoreError {
    StoreError::Unsupported(format!(
        "$filter: '{what}' wird vom relationalen Store nicht nach SQL uebersetzt"
    ))
}

pub struct Translator<'a> {
    set: &'a EntitySet,
    alias: &'a str,
    params: Vec<SqlValue>,
}

impl<'a> Translator<'a> {
    pub fn new(set: &'a EntitySet, alias: &'a str) -> Self {
        Self {
            set,
            alias,
            params: vec![],
        }
    }

    /// Uebersetzt einen Filterausdruck in ein `WHERE`-Fragment.
    pub fn filter(mut self, expr: &Expr) -> Result<Sql, StoreError> {
        let text = self.predicate(expr)?;
        Ok(Sql {
            text,
            params: self.params,
        })
    }

    fn column(&self, name: &str) -> Result<String, StoreError> {
        if name.contains('/') {
            return Err(unsupported(format!("Pfadausdruck {name}")));
        }
        // Existenz gegen das Schema pruefen: ein Tippfehler soll auffallen,
        // nicht als "kein Treffer" durchgehen.
        self.set
            .get_column(name)
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        Ok(format!("{}.{}", self.alias, quote(name)))
    }

    fn push(&mut self, value: SqlValue) -> String {
        self.params.push(value);
        format!("?{}", self.params.len())
    }

    /// Skalarer Ausdruck (Spalte, Literal, Funktion).
    fn scalar(&mut self, expr: &Expr) -> Result<String, StoreError> {
        Ok(match expr {
            Expr::Identifier(name) => self.column(name)?,
            Expr::Path(parts) => return Err(unsupported(parts.join("/"))),
            Expr::String(s) => self.push(SqlValue::Text(s.clone())),
            Expr::Guid(g) => self.push(SqlValue::Text(g.clone())),
            Expr::Number(n) => {
                if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                    self.push(SqlValue::Integer(*n as i64))
                } else {
                    self.push(SqlValue::Real(*n))
                }
            }
            Expr::Bool(b) => self.push(SqlValue::Integer(i64::from(*b))),
            Expr::Null => "NULL".to_string(),
            Expr::FunctionCall { name, args } => self.function(name, args)?,
            Expr::UnaryNot(_) | Expr::BinaryOp { .. } => self.predicate(expr)?,
        })
    }

    fn function(&mut self, name: &str, args: &[Expr]) -> Result<String, StoreError> {
        let arg = |this: &mut Self, i: usize| -> Result<String, StoreError> {
            match args.get(i) {
                Some(e) => this.scalar(e),
                None => Err(unsupported(format!("{name}: fehlendes Argument {i}"))),
            }
        };
        let lower = name.to_ascii_lowercase();
        Ok(match lower.as_str() {
            "contains" => {
                let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                format!("instr({a}, {b}) > 0")
            }
            // Ueber substr statt LIKE: kein Escaping von % und _ noetig.
            "startswith" => {
                let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                format!("substr({a}, 1, length({b})) = {b}")
            }
            "endswith" => {
                let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                format!("substr({a}, -length({b})) = {b}")
            }
            // OData V2: vertauschte Argumente
            "substringof" => {
                let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                format!("instr({b}, {a}) > 0")
            }
            "tolower" => format!("lower({})", arg(self, 0)?),
            "toupper" => format!("upper({})", arg(self, 0)?),
            "trim" => format!("trim({})", arg(self, 0)?),
            "length" => format!("length({})", arg(self, 0)?),
            "indexof" => {
                let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                format!("(instr({a}, {b}) - 1)")
            }
            "concat" => {
                let mut parts = vec![];
                for i in 0..args.len() {
                    parts.push(arg(self, i)?);
                }
                format!("({})", parts.join(" || "))
            }
            "substring" => {
                let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                if args.len() > 2 {
                    let c = arg(self, 2)?;
                    format!("substr({a}, {b} + 1, {c})")
                } else {
                    format!("substr({a}, {b} + 1)")
                }
            }
            "round" => format!("round({})", arg(self, 0)?),
            // floor/ceiling kennt SQLite ohne Erweiterung nicht. Lieber ein
            // klarer Fehler als eine Emulation, die am Dokumentenstore
            // vorbeirundet.
            other => return Err(unsupported(format!("Funktion {other}"))),
        })
    }

    /// Boolescher Ausdruck.
    fn predicate(&mut self, expr: &Expr) -> Result<String, StoreError> {
        Ok(match expr {
            Expr::BinaryOp { op, left, right } => match op {
                BinOp::And | BinOp::Or => {
                    let keyword = if matches!(op, BinOp::And) { "AND" } else { "OR" };
                    format!(
                        "({} {} {})",
                        self.predicate(left)?,
                        keyword,
                        self.predicate(right)?
                    )
                }
                BinOp::Eq | BinOp::Ne => {
                    // NULL-Vergleiche brauchen IS / IS NOT.
                    let null_cmp = matches!(**left, Expr::Null) || matches!(**right, Expr::Null);
                    let sql_op = match (op, null_cmp) {
                        (BinOp::Eq, true) => "IS",
                        (BinOp::Eq, false) => "=",
                        (BinOp::Ne, true) => "IS NOT",
                        (BinOp::Ne, false) => "<>",
                        _ => unreachable!(),
                    };
                    format!(
                        "({} {} {})",
                        self.scalar(left)?,
                        sql_op,
                        self.scalar(right)?
                    )
                }
                BinOp::Gt | BinOp::Ge | BinOp::Lt | BinOp::Le => {
                    let sql_op = match op {
                        BinOp::Gt => ">",
                        BinOp::Ge => ">=",
                        BinOp::Lt => "<",
                        BinOp::Le => "<=",
                        _ => unreachable!(),
                    };
                    format!(
                        "({} {} {})",
                        self.scalar(left)?,
                        sql_op,
                        self.scalar(right)?
                    )
                }
            },
            Expr::UnaryNot(inner) => format!("(NOT {})", self.predicate(inner)?),
            Expr::Bool(b) => (if *b { "1 = 1" } else { "1 = 0" }).to_string(),
            Expr::FunctionCall { name, args } => self.function(name, args)?,
            Expr::Identifier(name) => format!("{} = 1", self.column(name)?),
            other => return Err(unsupported(format!("{other:?}"))),
        })
    }
}

/// `ORDER BY`-Klausel. Leere Sortierung ergibt `None`.
pub fn order_by_clause(
    set: &EntitySet,
    alias: &str,
    query: &ODataQuery,
) -> Result<Option<String>, StoreError> {
    if query.orderby.is_empty() {
        return Ok(None);
    }
    let mut parts = vec![];
    for item in query.orderby.iter() {
        if item.property.contains('/') {
            return Err(unsupported(format!("$orderby {}", item.property)));
        }
        set.get_column(&item.property)
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        let direction = match item.direction {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        };
        // SQLite sortiert NULL aufsteigend zuerst - gleiche Ordnung wie
        // `data_store::query::compare_json`.
        parts.push(format!(
            "{}.{} {}",
            alias,
            quote(&item.property),
            direction
        ));
    }
    Ok(Some(parts.join(", ")))
}

/// `LIMIT`/`OFFSET`. SQLite verlangt ein LIMIT, wenn ein OFFSET gesetzt ist.
pub fn paging_clause(query: &ODataQuery) -> String {
    match (query.top, query.skip) {
        (None, None) => String::new(),
        (Some(top), None) => format!(" LIMIT {top}"),
        (None, Some(skip)) => format!(" LIMIT -1 OFFSET {skip}"),
        (Some(top), Some(skip)) => format!(" LIMIT {top} OFFSET {skip}"),
    }
}

/// Hilfsfunktion fuer `IN (...)`-Listen.
pub fn placeholders(count: usize, offset: usize) -> String {
    (0..count)
        .map(|i| format!("?{}", i + offset + 1))
        .collect::<Vec<_>>()
        .join(", ")
}

/// JSON-Wert als Parameter, ohne Spaltenkontext.
pub fn param(value: &serde_json::Value) -> SqlValue {
    json_to_sql_loose(value)
}
