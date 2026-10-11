//! Uebersetzung von `$filter` und `$orderby` nach SQL.
//!
//! Zwei Regeln bestimmen das Design:
//!
//! 1. **Nie interpolieren.** Werte gehen ausschliesslich als gebundene
//!    Parameter in die Anweisung.
//! 2. **Nie stillschweigend abweichen.** Was nicht uebersetzbar ist, liefert
//!    [`StoreError`] - ein Filter, der klammheimlich ignoriert wird, gibt
//!    plausible falsche Daten zurueck, und das ist schlimmer als ein Fehler.

use data_store::{json_to_param_loose, SqlParam, StoreError};
use odata::query::ODataQuery;

use crate::dialect::Dialect;

/// Ergebnis einer Uebersetzung: SQL-Fragment plus zugehoerige Parameter.
///
/// Die Parameter sind treiberunabhaengig; erst der Store rechnet sie per
/// [`crate::value::to_sqlite`] an der rusqlite-Grenze um.
pub struct Sql {
    pub text: String,
    pub params: Vec<SqlParam>,
}

fn unsupported(what: impl std::fmt::Display) -> StoreError {
    StoreError::Unsupported(format!(
        "$filter: '{what}' wird vom relationalen Store nicht nach SQL uebersetzt"
    ))
}

/// `LIMIT`/`OFFSET` in der Schreibweise des Dialekts.
pub fn paging_clause(dialect: &dyn Dialect, query: &ODataQuery) -> String {
    dialect.paging(query.top, query.skip)
}

/// Hilfsfunktion fuer `IN (...)`-Listen.
pub fn placeholders(dialect: &dyn Dialect, count: usize, offset: usize) -> String {
    (0..count)
        .map(|i| dialect.placeholder(i + offset + 1))
        .collect::<Vec<_>>()
        .join(", ")
}

/// JSON-Wert als Parameter, ohne Spaltenkontext.
pub fn param(value: &serde_json::Value) -> SqlParam {
    json_to_param_loose(value)
}

pub mod query {
    use super::unsupported;
    use super::Sql;
    use data_store::schema::{
        Cardinality, EntitySet, ForeignKeySide, Schema, ID_COLUMN, IS_ACTIVE_COLUMN,
    };
    use data_store::{SqlParam, StoreError};
    use odata::query::{BinOp, Expr, LambdaKind, ODataQuery};

    use crate::ddl::{quote, DRAFT_ADMIN_COLUMN};
    use crate::dialect::Dialect;

    /// Spalte der Sicht, die den Aktivierungszustand des Zwillings traegt.
    pub const SIBLING_IS_ACTIVE: &str = "SiblingEntity/IsActiveEntity";
    /// Draft-Verwaltungsdaten des Zwillings.
    pub const SIBLING_DRAFT_ADMIN: &str = "SiblingEntity/DraftAdministrativeData_ID";
    /// Alias der aeusseren Zeile. Lambda-Ausdrucke verweisen darauf korreliert;
    /// der Alias steht nur im SQL, wenn ein Lambda ihn tatsaechlich braucht.
    const ROOT_ALIAS: &str = "root";

    /// Lambda-Variable: ihr Name, der Alias des Elements in der Unterabfrage
    /// und das Set, zu dem das Element gehoert.
    #[derive(Debug, Clone)]
    struct LambdaScope<'a> {
        var: String,
        alias: String,
        set: &'a EntitySet,
    }

    /// Sichtbarkeitsregel des Draft-Modells: ein Draft verdeckt seinen aktiven
    /// Zwilling.
    ///
    /// Formuliert als Praedikat auf der Sibling-Sicht, nicht als korrelierte
    /// Unterabfrage - die Information steht durch den Join bereits in der
    /// Zeile. Die Regel ist wortgleich mit dem Draft-Filter, den Fiori Elements
    /// selbst schickt, und daher idempotent: kommt sie doppelt an, aendert das
    /// am Ergebnis nichts.
    ///
    /// Nebeneffekt, auf den Verlass ist: Zeilen, die der `LEFT JOIN` nur
    /// von der rechten Seite beisteuert, haben `IsActiveEntity IS NULL`. Fuer
    /// sie ist der erste Zweig NULL und der zweite falsch - sie fallen heraus.
    fn shadow_predicate(dialect: &dyn Dialect) -> String {
        format!(
            "({is_draft} OR {sibling} IS NULL)",
            is_draft = dialect.bool_predicate(&quote(IS_ACTIVE_COLUMN), false),
            sibling = quote(SIBLING_IS_ACTIVE),
        )
    }

    #[derive(Debug)]
    pub struct Translator<'a> {
        set: &'a EntitySet,
        dialect: &'a dyn Dialect,
        /// Noetig, um Navigationen in Lambda-Ausdruecken aufzuloesen.
        schema: Option<&'a Schema>,
        params: Vec<SqlParam>,
        filter: Vec<Expr>,
        raw_filters: Vec<String>,
        select: Vec<String>,
        /// Innerhalb eines Lambda-Ausdrucks gesetzt.
        scope: Option<LambdaScope<'a>>,
        aliases: usize,
        root_referenced: bool,
    }

    impl<'a> Translator<'a> {
        pub fn new(set: &'a EntitySet, dialect: &'a dyn Dialect) -> Self {
            Self {
                set,
                dialect,
                schema: None,
                params: vec![],
                filter: vec![],
                raw_filters: vec![],
                select: vec![],
                scope: None,
                aliases: 0,
                root_referenced: false,
            }
        }

        /// Schema bereitstellen. Ohne Schema werden Lambda-Ausdruecke abgelehnt.
        pub fn with_schema(mut self, schema: &'a Schema) -> Self {
            self.schema = Some(schema);
            self
        }

        pub fn build(mut self) -> Result<Sql, StoreError> {
            // Reihenfolge: erst die Ausdrucksfilter (sie binden Parameter),
            // dann die bereits umnummerierten Rohfilter.
            let exprs = std::mem::take(&mut self.filter);
            let mut conditions: Vec<String> = Vec::with_capacity(exprs.len());
            for expr in &exprs {
                conditions.push(self.predicate(expr)?);
            }
            conditions.extend(self.raw_filters.iter().cloned());
            let condition_sql = conditions.join(" AND ");

            let fields_sql = if self.select.is_empty() {
                "*".to_string()
            } else {
                self.select
                    .iter()
                    .map(|c| quote(c))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let inner_sql = self.build_inner()?;

            let root_alias = if self.root_referenced {
                format!(" AS {}", quote(ROOT_ALIAS))
            } else {
                String::new()
            };
            let stmt_sql = if condition_sql.is_empty() {
                format!("SELECT {} FROM ({}){}", fields_sql, inner_sql, root_alias)
            } else {
                format!(
                    "SELECT {} FROM ({}){} WHERE {}",
                    fields_sql, inner_sql, root_alias, condition_sql
                )
            }
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
            Ok(Sql {
                text: stmt_sql,
                params: self.params,
            })
        }

        /// Nimmt einen Filterausdruck auf. Uebersetzt wird erst in [`build`],
        /// damit Parameter in stabiler Reihenfolge gebunden werden.
        pub fn filter(mut self, expr: &Expr) -> Self {
            self.filter.push(expr.clone());
            self
        }

        /// Fuegt ein bereits fertiges `WHERE`-Fragment mit seinen Parametern
        /// hinzu. Die Platzhalter im Fragment muessen die Form `?N` mit
        /// bereits konsistenter Nummerierung haben -- typischerweise wird der
        /// Aufrufer sie beim Zusammenbau mit [`super::placeholders`] erzeugen.
        pub fn filter_raw(mut self, sql: impl Into<String>, params: Vec<SqlParam>) -> Self {
            let sql = sql.into();
            // Platzhalter im Fragment relativ zur laufenden Parameterliste
            // umnummerieren, damit mehrere raw-Filter kombinierbar sind.
            let mut offset = self.params.len();
            let renumbered = renumber_placeholders(&sql, self.dialect, &mut offset);
            self.params.extend(params);
            self.raw_filters.push(renumbered);
            self
        }

        /// Blendet aktive Saetze aus, zu denen ein Draft existiert.
        ///
        /// Fuer Sets ohne Draft-Unterstuetzung wirkungslos.
        pub fn shadow_drafts(mut self) -> Self {
            if self.set.draft_enabled {
                self.raw_filters.push(shadow_predicate(self.dialect));
            }
            self
        }

        fn column(&self, name: &str) -> Result<String, StoreError> {
            // Existenz gegen das Schema pruefen: ein Tippfehler soll auffallen,
            // nicht als "kein Treffer" durchgehen.
            self.set
                .get_column(name)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            Ok(quote(name))
        }

        fn require_schema(&self) -> Result<&'a Schema, StoreError> {
            self.schema
                .ok_or_else(|| unsupported("Lambda-Ausdruck ohne Schema".to_string()))
        }

        fn fresh_alias(&mut self) -> String {
            self.aliases += 1;
            format!("l{}", self.aliases)
        }

        /// Feld eines Ausdrucks. Innerhalb eines Lambda-Ausdrucks beginnt ein
        /// Pfad mit der Lambda-Variable beim Element, alles andere gehoert zur
        /// aeusseren Zeile.
        fn field(&mut self, parts: &[String]) -> Result<String, StoreError> {
            if let Some(scope) = self.scope.clone() {
                if let Some((head, rest)) = parts.split_first() {
                    if *head == scope.var {
                        return self.element_value(&scope.alias, scope.set, rest);
                    }
                }
            }
            let plain = match parts {
                [name] => self.column(name)?,
                _ => self.sibling_column(parts)?,
            };
            if self.scope.is_some() {
                self.root_referenced = true;
                return Ok(format!("{}.{}", quote(ROOT_ALIAS), plain));
            }
            Ok(plain)
        }

        /// Wert eines Lambda-Elements entlang eines Pfads. Jede to-one-Navigation
        /// wird zu einer korrelierten Skalar-Unterabfrage.
        fn element_value(
            &mut self,
            alias: &str,
            set: &'a EntitySet,
            path: &[String],
        ) -> Result<String, StoreError> {
            let (head, rest) = path
                .split_first()
                .ok_or_else(|| unsupported("Lambda-Variable ohne Feld".to_string()))?;
            if rest.is_empty() {
                set.get_column(head)
                    .map_err(|e| StoreError::Metadata(e.to_string()))?;
                return Ok(format!("{}.{}", quote(alias), quote(head)));
            }
            let assoc = set
                .get_association(head)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            if assoc.cardinality != Cardinality::ToOne {
                return Err(unsupported(format!("Pfad ueber Collection {head}")));
            }
            let ForeignKeySide::Here { column } = &assoc.foreign_key else {
                return Err(unsupported(format!(
                    "Navigation {head} ohne lokalen Fremdschluessel"
                )));
            };
            let target = self
                .require_schema()?
                .set(&assoc.target_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;

            let target_alias = self.fresh_alias();
            let value = self.element_value(&target_alias, target, rest)?;
            let mut join = format!(
                "{}.{} = {}.{}",
                quote(&target_alias),
                quote(ID_COLUMN),
                quote(alias),
                quote(column)
            );
            if target.draft_enabled && set.draft_enabled {
                join.push_str(&format!(
                    " AND {}.{} = {}.{}",
                    quote(&target_alias),
                    quote(IS_ACTIVE_COLUMN),
                    quote(alias),
                    quote(IS_ACTIVE_COLUMN)
                ));
            }
            Ok(format!(
                "(SELECT {value} FROM {} AS {} WHERE {join})",
                quote(&target.name),
                quote(&target_alias)
            ))
        }

        /// `kind` ueber die Kinder einer to-many-Navigation als Korrelation.
        ///
        /// Dreiwertig wie der Referenzevaluator: `any` ist wahr, sobald ein Kind
        /// wahr ist; sonst unbekannt, sobald ein Kind unbekannt ist; sonst falsch.
        /// `all` spiegelbildlich. Leere Kindmenge: `any` falsch, `all` wahr.
        fn lambda(
            &mut self,
            kind: LambdaKind,
            collection: &[String],
            var: &str,
            predicate: &Expr,
        ) -> Result<String, StoreError> {
            if self.scope.is_some() {
                return Err(unsupported("verschachtelte Lambda-Ausdruecke".to_string()));
            }
            let [nav] = collection else {
                return Err(unsupported(format!(
                    "Lambda ueber Pfad {}",
                    collection.join("/")
                )));
            };
            let set = self.set;
            let assoc = set
                .get_association(nav)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            if assoc.cardinality != Cardinality::ToMany {
                return Err(unsupported(format!("Lambda ueber to-one Navigation {nav}")));
            }
            let ForeignKeySide::There { column } = &assoc.foreign_key else {
                return Err(unsupported(format!(
                    "Collection {nav} ohne Fremdschluessel im Kind"
                )));
            };
            let child = self
                .require_schema()?
                .set(&assoc.target_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;

            let alias = self.fresh_alias();
            self.root_referenced = true;
            let root = quote(ROOT_ALIAS);
            let mut join = format!(
                "{}.{} = {}.{}",
                quote(&alias),
                quote(column),
                root,
                quote(ID_COLUMN)
            );
            if child.draft_enabled && set.draft_enabled {
                join.push_str(&format!(
                    " AND {}.{} = {}.{}",
                    quote(&alias),
                    quote(IS_ACTIVE_COLUMN),
                    root,
                    quote(IS_ACTIVE_COLUMN)
                ));
            }
            let from = format!("{} AS {}", quote(&child.name), quote(&alias));

            self.scope = Some(LambdaScope {
                var: var.to_string(),
                alias: alias.clone(),
                set: child,
            });
            let body = self.predicate(predicate);
            self.scope = None;
            let body = body?;

            let true_lit = self.dialect.bool_constant(true);
            let false_lit = self.dialect.bool_constant(false);
            let unknown = format!("EXISTS (SELECT 1 FROM {from} WHERE {join} AND ({body}) IS NULL)");
            Ok(match kind {
                LambdaKind::Any => {
                    let hit = format!("EXISTS (SELECT 1 FROM {from} WHERE {join} AND ({body}))");
                    format!("CASE WHEN {hit} THEN {true_lit} WHEN {unknown} THEN NULL ELSE {false_lit} END")
                }
                LambdaKind::All => {
                    let miss = format!("EXISTS (SELECT 1 FROM {from} WHERE {join} AND NOT ({body}))");
                    format!("CASE WHEN {miss} THEN {false_lit} WHEN {unknown} THEN NULL ELSE {true_lit} END")
                }
            })
        }

        fn push(&mut self, value: SqlParam) -> String {
            self.params.push(value);
            self.dialect.placeholder(self.params.len())
        }

        /// Skalarer Ausdruck (Spalte, Literal, Funktion).
        fn scalar(&mut self, expr: &Expr) -> Result<String, StoreError> {
            Ok(match expr {
                Expr::Identifier(name) => self.field(std::slice::from_ref(name))?,
                Expr::Path(parts) => self.field(parts)?,
                Expr::String(s) => self.push(SqlParam::Text(s.clone())),
                Expr::Guid(g) => self.push(SqlParam::Text(g.clone())),
                Expr::Number(n) => {
                    if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                        self.push(SqlParam::Int(*n as i64))
                    } else {
                        self.push(SqlParam::Real(*n))
                    }
                }
                Expr::Bool(b) => self.push(SqlParam::Bool(*b)),
                Expr::Null => "NULL".to_string(),
                Expr::FunctionCall { name, args } => self.function(name, args)?,
                Expr::UnaryNot(_) | Expr::BinaryOp { .. } => self.predicate(expr)?,
                Expr::Lambda { .. } => return Err(unsupported("Lambda-Ausdruck".to_string())),
            })
        }

        /// Pfadausdruecke sind auf die Spalten der Sibling-Sicht begrenzt.
        ///
        /// `SiblingEntity/IsActiveEntity` und
        /// `SiblingEntity/DraftAdministrativeData_ID` stellt der Self-Join
        /// bereit. Jeder andere Pfad braeuchte einen zusaetzlichen Join und
        /// wird abgelehnt, statt still ins Leere zu laufen.
        fn sibling_column(&self, parts: &[String]) -> Result<String, StoreError> {
            let path = parts.join("/");
            match path.as_str() {
                SIBLING_IS_ACTIVE | SIBLING_DRAFT_ADMIN => Ok(quote(&path)),
                _ => Err(unsupported(format!("Pfadausdruck {path}"))),
            }
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
                    format!("{} > 0", self.dialect.position(&a, &b))
                }
                // Ueber substr statt LIKE: kein Escaping von % und _ noetig.
                "startswith" => {
                    let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                    format!("substr({a}, 1, length({b})) = {b}")
                }
                "endswith" => {
                    let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                    self.dialect.ends_with(&a, &b)
                }
                // OData V2: vertauschte Argumente
                "substringof" => {
                    let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                    format!("{} > 0", self.dialect.position(&b, &a))
                }
                "tolower" => format!("lower({})", arg(self, 0)?),
                "toupper" => format!("upper({})", arg(self, 0)?),
                "trim" => format!("trim({})", arg(self, 0)?),
                "length" => format!("length({})", arg(self, 0)?),
                "indexof" => {
                    let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                    format!("({} - 1)", self.dialect.position(&a, &b))
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
                // floor/ceiling bleibt aussen vor: SQLite kennt es ohne
                // Erweiterung nicht, und ein Backend, das mehr kann als die
                // anderen, macht die geteilte Verhaltenssuite wertlos.
                other => return Err(unsupported(format!("Funktion {other}"))),
            })
        }

        /// Boolescher Ausdruck.
        fn predicate(&mut self, expr: &Expr) -> Result<String, StoreError> {
            Ok(match expr {
                Expr::BinaryOp { op, left, right } => match op {
                    BinOp::And | BinOp::Or => {
                        let keyword = if matches!(op, BinOp::And) {
                            "AND"
                        } else {
                            "OR"
                        };
                        format!(
                            "({} {} {})",
                            self.predicate(left)?,
                            keyword,
                            self.predicate(right)?
                        )
                    }
                    BinOp::Eq | BinOp::Ne => {
                        // NULL-Vergleiche brauchen IS / IS NOT.
                        let null_cmp =
                            matches!(**left, Expr::Null) || matches!(**right, Expr::Null);
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
                Expr::Bool(b) => self.dialect.bool_constant(*b),
                Expr::FunctionCall { name, args } => self.function(name, args)?,
                Expr::Identifier(name) => {
                    let column = self.field(std::slice::from_ref(name))?;
                    self.dialect.bool_predicate(&column, true)
                }
                Expr::Lambda {
                    kind,
                    collection,
                    var,
                    predicate,
                } => self.lambda(*kind, collection, var, predicate)?,
                other => return Err(unsupported(format!("{other:?}"))),
            })
        }

        pub fn select(mut self, arg: &str) -> Self {
            self.select.push(arg.to_string());
            self
        }

        pub fn select_is_empty(&self) -> bool {
            self.select.is_empty()
        }

        fn build_inner(&self) -> Result<String, StoreError> {
            if !self.set.draft_enabled {
                return self.build_base();
            }
            let base_sql = self.build_base()?;
            // Draft-Flags aus dem Sibling-Join ableiten. `SiblingEntity/IsActiveEntity`
            // ist:
            //   NULL  -> kein Zwilling
            //   1     -> Zwilling ist aktiv  (also bin ich der Draft) -> HasActiveEntity
            //   0     -> Zwilling ist Draft  (also bin ich aktiv)     -> HasDraftEntity
            // Wichtig: die vorherige Formel `1 - COALESCE(sibling, 0)` war falsch,
            // weil sie NULL (kein Zwilling) zu `HasDraftEntity=1` machte.
            let sibling = quote(SIBLING_IS_ACTIVE);
            Ok(format!(
                r#"SELECT *,
                     CASE WHEN {sibling_is_draft} OR {is_draft} THEN {yes} ELSE {no} END AS {has_draft},
                     CASE WHEN {sibling_is_active} THEN {yes} ELSE {no} END AS {has_active}
                   FROM ( {base} )"#,
                is_draft = self.dialect.bool_predicate(&quote(IS_ACTIVE_COLUMN), false),
                sibling_is_draft = self.dialect.bool_predicate(&sibling, false),
                sibling_is_active = self.dialect.bool_predicate(&sibling, true),
                yes = self.dialect.bool_literal(true),
                no = self.dialect.bool_literal(false),
                has_draft = quote("HasDraftEntity"),
                has_active = quote("HasActiveEntity"),
                base = base_sql,
            ))
        }

        /// Basissicht: alle Zeilen des Sets, angereichert um den Zustand ihres
        /// Zwillings.
        ///
        /// Bezeichner werden durchgehend gequotet. Ohne Quoting waeren die
        /// Namen dialektabhaengig (SQLite ist tolerant, andere Datenbanken
        /// falten unquotierte Bezeichner auf Klein- oder Grossschreibung),
        /// waehrend die Tabellen per [`crate::ddl`] gequotet und damit
        /// gross-/kleinschreibungsgenau angelegt werden.
        fn build_base(&self) -> Result<String, StoreError> {
            let table = quote(&self.set.name);
            if !self.set.draft_enabled {
                return Ok(format!("SELECT * FROM {table}"));
            }
            Ok(format!(
                r#"SELECT a.*,
                     d.{is_active} AS {sib_active},
                     d.{admin} AS {sib_admin}
                   FROM {table} AS a
                   LEFT JOIN {table} AS d
                        ON a.{id} = d.{id} AND a.{is_active} <> d.{is_active}"#,
                id = quote(ID_COLUMN),
                is_active = quote(IS_ACTIVE_COLUMN),
                admin = quote(DRAFT_ADMIN_COLUMN),
                sib_active = quote(SIBLING_IS_ACTIVE),
                sib_admin = quote(SIBLING_DRAFT_ADMIN),
                table = table,
            ))
        }
    }

    /// Nummeriert die Platzhalter in `sql` neu, beginnend bei `*offset + 1`,
    /// und aktualisiert `offset` auf den zuletzt verwendeten Wert. So laesst
    /// sich ein vorformuliertes Fragment sicher in eine bestehende
    /// Parameterliste einbetten.
    ///
    /// Erkannt wird die Platzhalterform des Dialekts: `?N` (SQLite) bzw. `$N`
    /// (Postgres).
    fn renumber_placeholders(sql: &str, dialect: &dyn Dialect, offset: &mut usize) -> String {
        let prefix = dialect
            .placeholder(1)
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .to_string();
        let mut result = String::with_capacity(sql.len());
        let mut rest = sql;
        while let Some(pos) = rest.find(&prefix) {
            result.push_str(&rest[..pos]);
            let after = &rest[pos + prefix.len()..];
            let digits = after.bytes().take_while(|b| b.is_ascii_digit()).count();
            if digits == 0 {
                // Kein Platzhalter, sondern zufaellig das Praefix.
                result.push_str(&prefix);
                rest = after;
                continue;
            }
            // Original-Index wird verworfen: die relative Reihenfolge im
            // Fragment ist massgebend, nicht der urspruengliche Wert.
            *offset += 1;
            result.push_str(&dialect.placeholder(*offset));
            rest = &after[digits..];
        }
        result.push_str(rest);
        result
    }

    /// `ORDER BY`-Klausel. Leere Sortierung ergibt `None`.
    ///
    /// Die Spalten werden ohne Tabellenalias referenziert. Das bleibt auch
    /// richtig, wenn das aeussere `SELECT ... FROM (...)` einen Alias traegt
    /// (bei Lambda-Ausdruecken): die Klausel hat nur diese eine Quelle.
    pub fn order_by_clause(
        set: &EntitySet,
        dialect: &dyn Dialect,
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
            // Die NULL-Ordnung gehoert zur Richtung: sie muss zu
            // `data_store::query::compare_json` passen, und die Datenbanken
            // sind sich darin nicht einig.
            parts.push(format!(
                "{} {}",
                quote(&item.property),
                dialect.order_direction(item.direction)
            ));
        }
        Ok(Some(parts.join(", ")))
    }

    #[cfg(test)]
    pub mod tests {
        use data_store::{Column, DataType, EntitySet};
        use odata::query::Expr;

        use crate::dialect::SQLITE;
        use crate::sql::query::Translator;

        fn order_set() -> EntitySet {
            EntitySet::new("OrderSet", "Order")
                .drafts()
                .column(Column::new("name", DataType::String { max_length: None }))
        }

        fn normalize(s: &str) -> String {
            s.split_whitespace().collect::<Vec<_>>().join(" ")
        }

        #[test]
        fn full_statement() -> anyhow::Result<()> {
            let set = order_set();
            let s = Translator::new(&set, &SQLITE)
                .select("ID")
                .select("IsActiveEntity")
                .select("HasActiveEntity")
                .select("HasDraftEntity")
                .select("name")
                .filter(
                    &Expr::parse(
                        "(IsActiveEntity eq false or SiblingEntity/IsActiveEntity eq null)",
                    )
                    .expect("parst"),
                )
                .build()?;

            let expected = normalize(
                r#"SELECT "ID", "IsActiveEntity", "HasActiveEntity", "HasDraftEntity", "name"
                   FROM (SELECT *,
                       CASE WHEN "SiblingEntity/IsActiveEntity" = 0 OR "IsActiveEntity" = 0 THEN 1 ELSE 0 END AS "HasDraftEntity",
                       CASE WHEN "SiblingEntity/IsActiveEntity" = 1 THEN 1 ELSE 0 END AS "HasActiveEntity"
                     FROM (
                       SELECT a.*,
                         d."IsActiveEntity" AS "SiblingEntity/IsActiveEntity",
                         d."DraftAdministrativeData_ID" AS "SiblingEntity/DraftAdministrativeData_ID"
                       FROM "OrderSet" AS a
                       LEFT JOIN "OrderSet" AS d
                         ON a."ID" = d."ID" AND a."IsActiveEntity" <> d."IsActiveEntity" ))
                   WHERE (("IsActiveEntity" = ?1) OR ("SiblingEntity/IsActiveEntity" IS NULL))"#,
            );
            assert_eq!(s.text, expected);
            Ok(())
        }

        /// Der Shadow-Filter ist wortgleich mit dem Draft-Filter, den Fiori
        /// selbst schickt - beide Wege muessen dasselbe Praedikat erzeugen.
        #[test]
        fn shadow_matches_the_fiori_draft_filter() -> anyhow::Result<()> {
            let set = order_set();
            let implicit = Translator::new(&set, &SQLITE).shadow_drafts().build()?;
            let explicit = Translator::new(&set, &SQLITE)
                .filter(
                    &Expr::parse(
                        "(IsActiveEntity eq false or SiblingEntity/IsActiveEntity eq null)",
                    )
                    .expect("parst"),
                )
                .build()?;
            assert!(
                implicit.text.ends_with(
                    r#"WHERE ("IsActiveEntity" = 0 OR "SiblingEntity/IsActiveEntity" IS NULL)"#
                ),
                "{}",
                implicit.text
            );
            assert!(explicit
                .text
                .contains(r#""SiblingEntity/IsActiveEntity" IS NULL"#));
            Ok(())
        }

        /// Sets ohne Drafts bekommen weder Sibling-Join noch Draft-Flags -
        /// die Spalte `IsActiveEntity` gibt es dort gar nicht.
        #[test]
        fn sets_without_drafts_skip_the_sibling_join() -> anyhow::Result<()> {
            let set = EntitySet::new("ThingSet", "Thing")
                .column(Column::new("name", DataType::String { max_length: None }));
            let s = Translator::new(&set, &SQLITE).build()?;
            assert_eq!(
                s.text,
                normalize(r#"SELECT * FROM (SELECT * FROM "ThingSet")"#)
            );
            Ok(())
        }

        /// Fehler muessen aus `build()` herauskommen, nicht als Panic.
        #[test]
        fn untranslatable_filters_return_an_error() {
            let set = order_set();
            for filter in ["floor(Amount) eq 1", "Nope eq 'x'", "Customer/Name eq 'x'"] {
                let expr = Expr::parse(filter).expect("parst");
                let result = Translator::new(&set, &SQLITE).filter(&expr).build();
                assert!(result.is_err(), "{filter} haette scheitern muessen");
            }
        }
    }
}
