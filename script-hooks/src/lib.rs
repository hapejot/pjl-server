//! Rhai-Skripte als Hooks pro Entity Set.
//!
//! Verzeichnis mit einer Datei `<set>.rhai` je Entity Set. Eine Datei definiert
//! beliebig viele der Funktionen `before_<op>` / `after_<op>` (Schreiben) und
//! `before_read` / `after_read` (Lesen). Fehlende Funktionen bedeuten "kein Hook".
//!
//! Jede Funktion bekommt ein `ctx`-Objekt und **muss es zurueckgeben**. Nur dann
//! werden Aenderungen uebernommen. Ein `throw "..."` bricht den Request ab.
//!
//! `ctx` enthaelt:
//! * `set`, `op`, `kind` (`"entity"`, `"list"`, `"property"`, `"write"`)
//! * `key` (`#{ ID, IsActiveEntity }` oder `()`)
//! * `property` (nur bei `kind == "property"`)
//! * `payload` (Datensatz als Map, Liste bei `kind == "list"`, sonst Wert)
//! * `query` (nur bei `before_read`): `$filter` als Struktur, `$top`, `$skip`, ...
//!
//! Hilfsfunktionen fuer Filter: `prop("Name")`, `text("x")`, `num(1)`,
//! `boolean(true)`, `eq(a, b)`, `ne`, `lt`, `le`, `gt`, `ge`, `and(a, b)`, `or(a, b)`,
//! `not(a)`. `and`/`or` mit `()` liefern die andere Seite, so laesst sich ein
//! Filter schrittweise ergaenzen: `ctx.query.filter = and(ctx.query.filter, ...)`.
//! Weitere Funktion: `now_iso()`.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use odata::query::ODataQuery;
use rhai::{AST, Dynamic, Engine, EvalAltResult, Scope};
use serde_json::{Value, json};

/// Wartezeit nach dem letzten Dateiereignis, bevor neu geladen wird.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(300);

/// Zeitlimit pro Skriptaufruf.
const CALL_TIMEOUT: Duration = Duration::from_secs(2);

thread_local! {
    /// Frist des laufenden Skriptaufrufs (Rhai laeuft synchron im aufrufenden Thread).
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("{0}")]
    Script(String),
}

struct Script {
    ast: AST,
    functions: HashSet<String>,
}

struct Inner {
    engine: Engine,
    dir: PathBuf,
    scripts: RwLock<HashMap<String, Arc<Script>>>,
    errors: RwLock<BTreeMap<String, String>>,
    reloads: AtomicU64,
}

/// Skript-Hooks eines Verzeichnisses. Beobachtet das Verzeichnis und laedt bei
/// Aenderungen neu. Ein fehlerhaftes Skript laesst die bisherige Version aktiv.
pub struct ScriptHooks {
    inner: Arc<Inner>,
    _watcher: Option<RecommendedWatcher>,
}

impl ScriptHooks {
    /// Laedt alle Skripte. Fehler beim Start brechen ab.
    pub fn load(dir: impl Into<PathBuf>) -> anyhow::Result<ScriptHooks> {
        let inner = Arc::new(Inner::new(dir.into()));
        inner.reload();
        let errors = inner.errors.read().unwrap().clone();
        if !errors.is_empty() {
            let list: Vec<String> = errors.iter().map(|(f, e)| format!("{f}: {e}")).collect();
            anyhow::bail!("script compile errors: {}", list.join("; "));
        }
        let dir = inner.dir.clone();
        // Debounce: Schreibvorgaenge (z. B. `fs::write` kuerzt die Datei zuerst)
        // erzeugen mehrere Ereignisse. Neu geladen wird erst nach einer Ruhephase,
        // sonst koennte eine leere Zwischenstufe als Skript gelten.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let watch_inner = inner.clone();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(RELOAD_DEBOUNCE).is_ok() {}
                watch_inner.reload();
            }
        });
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let relevant = event
                    .paths
                    .iter()
                    .any(|p| p.extension().is_some_and(|e| e == "rhai"));
                if relevant {
                    let _ = tx.send(());
                }
            }
        })?;
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        tracing::info!(dir = %dir.display(), "script hooks loaded");
        Ok(ScriptHooks {
            inner,
            _watcher: Some(watcher),
        })
    }

    /// Laedt alle Skripte neu. Gibt die Anzahl der Fehler zurueck.
    pub fn reload(&self) -> usize {
        self.inner.reload();
        self.inner.errors.read().unwrap().len()
    }

    /// Stand fuer den Status (Sets, Funktionen, Fehler).
    pub fn status(&self) -> Value {
        let scripts = self.inner.scripts.read().unwrap();
        let mut sets = serde_json::Map::new();
        for (name, script) in scripts.iter() {
            let mut fns: Vec<&String> = script.functions.iter().collect();
            fns.sort();
            sets.insert(name.clone(), json!(fns));
        }
        json!({
            "dir": self.inner.dir.display().to_string(),
            "sets": sets,
            "errors": *self.inner.errors.read().unwrap(),
            "reloads": self.inner.reloads.load(Ordering::Relaxed),
        })
    }

    /// Schreiboperation vor dem Store (`before_<op>`).
    pub fn before(&self, set: &str, op: &str, payload: &mut Value) -> Result<(), ScriptError> {
        let ctx = json!({"set": set, "op": op, "kind": "write", "key": null, "payload": payload});
        if let Some(res) = self.call(set, &format!("before_{op}"), ctx)? {
            take(&res, "payload", payload);
        }
        Ok(())
    }

    /// Schreiboperation nach dem Store (`after_<op>`).
    pub fn after(&self, set: &str, op: &str, payload: &mut Value) -> Result<(), ScriptError> {
        let ctx = json!({"set": set, "op": op, "kind": "write", "key": null, "payload": payload});
        if let Some(res) = self.call(set, &format!("after_{op}"), ctx)? {
            take(&res, "payload", payload);
        }
        Ok(())
    }

    /// Lesezugriff vor dem Store, einmal pro Request (`before_read`).
    pub fn before_read(
        &self,
        set: &str,
        key: Option<Value>,
        query: &mut ODataQuery,
    ) -> Result<(), ScriptError> {
        let kind = if key.is_some() { "entity" } else { "list" };
        let ctx = json!({
            "set": set,
            "op": "read",
            "kind": kind,
            "key": key.unwrap_or(Value::Null),
            "payload": null,
            "query": query_to_script(query)?,
        });
        if let Some(res) = self.call(set, "before_read", ctx)? {
            if let Some(q) = res.get("query") {
                *query = query_from_script(q)
                    .map_err(|e| ScriptError::Script(format!("{set}.before_read: query: {e}")))?;
            }
        }
        Ok(())
    }

    /// Lesezugriff nach dem Store, einmal pro Request (`after_read`).
    /// `kind` ist `"entity"`, `"list"` oder `"property"` (dann mit `property`).
    pub fn after_read(
        &self,
        set: &str,
        kind: &str,
        key: Option<Value>,
        property: Option<&str>,
        payload: &mut Value,
    ) -> Result<(), ScriptError> {
        let ctx = json!({
            "set": set,
            "op": "read",
            "kind": kind,
            "key": key.unwrap_or(Value::Null),
            "property": property,
            "payload": payload,
        });
        if let Some(res) = self.call(set, "after_read", ctx)? {
            take(&res, "payload", payload);
        }
        Ok(())
    }

    /// Ruft `fname` im Skript von `set` auf. `None`, wenn es die Funktion nicht gibt.
    fn call(&self, set: &str, fname: &str, ctx: Value) -> Result<Option<Value>, ScriptError> {
        let script = match self.inner.scripts.read().unwrap().get(set) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };
        if !script.functions.contains(fname) {
            return Ok(None);
        }
        let arg = rhai::serde::to_dynamic(&ctx)
            .map_err(|e| ScriptError::Script(format!("{set}.{fname}: {e}")))?;
        DEADLINE.with(|d| d.set(Some(Instant::now() + CALL_TIMEOUT)));
        let result = self
            .inner
            .engine
            .call_fn::<Dynamic>(&mut Scope::new(), &script.ast, fname, (arg,));
        DEADLINE.with(|d| d.set(None));
        let dynamic = result.map_err(|e| ScriptError::Script(format!("{set}.{fname}: {}", describe(&e))))?;
        let value: Value = rhai::serde::from_dynamic(&dynamic)
            .map_err(|e| ScriptError::Script(format!("{set}.{fname}: {e}")))?;
        if !value.is_object() {
            return Err(ScriptError::Script(format!(
                "{set}.{fname}: die Funktion muss das ctx-Objekt zurueckgeben"
            )));
        }
        Ok(Some(value))
    }
}

impl Inner {
    fn new(dir: PathBuf) -> Inner {
        Inner {
            engine: build_engine(),
            dir,
            scripts: RwLock::new(HashMap::new()),
            errors: RwLock::new(BTreeMap::new()),
            reloads: AtomicU64::new(0),
        }
    }

    /// Liest alle `*.rhai`-Dateien. Ein fehlerhaftes Skript behaelt seine bisherige Version.
    fn reload(&self) {
        let mut next: HashMap<String, Arc<Script>> = HashMap::new();
        let mut errors = BTreeMap::new();
        let old = self.scripts.read().unwrap().clone();

        match std::fs::read_dir(&self.dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_none_or(|e| e != "rhai") {
                        continue;
                    }
                    let Some(set) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string)
                    else {
                        continue;
                    };
                    match compile(&self.engine, &path) {
                        Ok(script) => {
                            next.insert(set, Arc::new(script));
                        }
                        Err(msg) => {
                            errors.insert(file_name(&path), msg.clone());
                            tracing::error!(file = %path.display(), "script not reloaded: {msg}");
                            if let Some(prev) = old.get(&set) {
                                next.insert(set, prev.clone());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                errors.insert(self.dir.display().to_string(), e.to_string());
            }
        }

        *self.scripts.write().unwrap() = next;
        *self.errors.write().unwrap() = errors;
        self.reloads.fetch_add(1, Ordering::Relaxed);
    }
}

fn compile(engine: &Engine, path: &Path) -> Result<Script, String> {
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let ast = engine.compile(&src).map_err(|e| e.to_string())?;
    let functions = ast
        .iter_functions()
        .map(|f| f.name.to_string())
        .collect::<HashSet<_>>();
    Ok(Script { ast, functions })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Schluessel der Query fuer Skripte: `$filter` heisst dort `filter` usw.
const QUERY_KEYS: [(&str, &str); 8] = [
    ("$filter", "filter"),
    ("$orderby", "orderby"),
    ("$top", "top"),
    ("$skip", "skip"),
    ("$select", "select"),
    ("$expand", "expand"),
    ("$count", "count"),
    ("$search", "search"),
];

fn query_to_script(query: &ODataQuery) -> Result<Value, ScriptError> {
    let mut v = to_value(query)?;
    rename_keys(&mut v, &QUERY_KEYS.map(|(a, b)| (a, b)));
    Ok(v)
}

fn query_from_script(v: &Value) -> Result<ODataQuery, String> {
    let mut v = v.clone();
    let reversed = QUERY_KEYS.map(|(a, b)| (b, a));
    rename_keys(&mut v, &reversed);
    serde_json::from_value(v).map_err(|e| e.to_string())
}

fn rename_keys(v: &mut Value, pairs: &[(&str, &str)]) {
    if let Value::Object(map) = v {
        for (from, to) in pairs {
            if let Some(val) = map.remove(*from) {
                map.insert((*to).to_string(), val);
            }
        }
    }
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value, ScriptError> {
    serde_json::to_value(v).map_err(|e| ScriptError::Script(e.to_string()))
}

/// Uebernimmt `res[key]` in `target`, falls vorhanden.
fn take(res: &Value, key: &str, target: &mut Value) {
    if let Some(v) = res.get(key) {
        *target = v.clone();
    }
}

fn describe(e: &EvalAltResult) -> String {
    match e {
        EvalAltResult::ErrorRuntime(d, _) => d.to_string(),
        other => other.to_string(),
    }
}

fn build_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(1_000_000);
    engine.set_max_call_levels(32);
    engine.set_max_string_size(1024 * 1024);
    engine.set_max_array_size(100_000);
    engine.set_max_map_size(10_000);
    engine.on_progress(|_| {
        DEADLINE.with(|d| match d.get() {
            Some(t) if Instant::now() > t => Some(Dynamic::from("Zeitlimit ueberschritten")),
            _ => None,
        })
    });
    register_helpers(&mut engine);
    engine
}

/// Baut einen Ausdruck im serde-Format von `odata::query::Expr`.
fn expr_binary(op: &str, a: Dynamic, b: Dynamic) -> Dynamic {
    let parse = |d: Dynamic| -> Value {
        if d.is_unit() {
            Value::Null
        } else {
            rhai::serde::from_dynamic(&d).unwrap_or(Value::Null)
        }
    };
    let (a, b) = (parse(a), parse(b));
    // `and`/`or` mit fehlender Seite liefern die andere Seite.
    if matches!(op, "And" | "Or") {
        if a.is_null() {
            return rhai::serde::to_dynamic(&b).unwrap_or(Dynamic::UNIT);
        }
        if b.is_null() {
            return rhai::serde::to_dynamic(&a).unwrap_or(Dynamic::UNIT);
        }
    }
    let v = json!({"BinaryOp": {"op": op, "left": a, "right": b}});
    rhai::serde::to_dynamic(&v).unwrap_or(Dynamic::UNIT)
}

fn to_dyn(v: Value) -> Dynamic {
    rhai::serde::to_dynamic(&v).unwrap_or(Dynamic::UNIT)
}

fn register_helpers(engine: &mut Engine) {
    engine.register_fn("prop", |name: &str| {
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() == 1 {
            to_dyn(json!({"Identifier": name}))
        } else {
            to_dyn(json!({"Path": parts}))
        }
    });
    engine.register_fn("text", |s: &str| to_dyn(json!({"String": s})));
    engine.register_fn("num", |n: f64| to_dyn(json!({"Number": n})));
    engine.register_fn("num", |n: i64| to_dyn(json!({"Number": n as f64})));
    engine.register_fn("boolean", |b: bool| to_dyn(json!({"Bool": b})));
    engine.register_fn("eq", |a: Dynamic, b: Dynamic| expr_binary("Eq", a, b));
    engine.register_fn("ne", |a: Dynamic, b: Dynamic| expr_binary("Ne", a, b));
    engine.register_fn("lt", |a: Dynamic, b: Dynamic| expr_binary("Lt", a, b));
    engine.register_fn("le", |a: Dynamic, b: Dynamic| expr_binary("Le", a, b));
    engine.register_fn("gt", |a: Dynamic, b: Dynamic| expr_binary("Gt", a, b));
    engine.register_fn("ge", |a: Dynamic, b: Dynamic| expr_binary("Ge", a, b));
    engine.register_fn("and", |a: Dynamic, b: Dynamic| expr_binary("And", a, b));
    engine.register_fn("or", |a: Dynamic, b: Dynamic| expr_binary("Or", a, b));
    engine.register_fn("not", |a: Dynamic| {
        let inner: Value = rhai::serde::from_dynamic(&a).unwrap_or(Value::Null);
        to_dyn(json!({"UnaryNot": inner}))
    });
    engine.register_fn("now_iso", || {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    });
}

#[cfg(test)]
mod tests;
