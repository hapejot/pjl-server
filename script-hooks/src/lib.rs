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

use chrono::TimeZone;
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use file_repo::{DirRepository, FileRepository};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use odata::query::ODataQuery;
use rhai::{Dynamic, Engine, EvalAltResult, Scope, AST};
use serde_json::{json, Value};
use tracing::info;

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

/// Komponenten-Skript (`components/<name>.rhai`), das die DDIC an Entitaeten bindet.
struct Binding {
    /// Schluessel des Komponenten-Skripts, z. B. `components/TimeRange`.
    script: String,
    /// Konfiguration aus der DDIC (Feldzuordnung, Schrittweite, ...).
    config: Value,
}

struct Inner {
    engine: Engine,
    /// Quelle der Skripte (Verzeichnis oder ZIP).
    repo: Arc<dyn FileRepository>,
    /// Anzeigename der Quelle fuer Fehler und Status.
    label: String,
    scripts: RwLock<HashMap<String, Arc<Script>>>,
    /// Komponenten-Skripte, Schluessel `components/<name>`.
    components: RwLock<HashMap<String, Arc<Script>>>,
    /// Entitaetsset -> gebundene Komponenten (laufen vor dem Entitaetsskript).
    bindings: RwLock<HashMap<String, Vec<Binding>>>,
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
        let dir: PathBuf = dir.into();
        let inner = Arc::new(Inner::new(
            Arc::new(DirRepository::new(&dir)),
            dir.display().to_string(),
        ));
        inner.reload();
        check_errors(&inner)?;
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
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
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

    /// Laedt Skripte aus einer beliebigen Quelle (z. B. ZIP im Speicher).
    /// Ohne Beobachtung: Aenderungen gibt es nur per `reload()`.
    pub fn load_from(
        repo: Arc<dyn FileRepository>,
        label: impl Into<String>,
    ) -> anyhow::Result<ScriptHooks> {
        let inner = Arc::new(Inner::new(repo, label.into()));
        inner.reload();
        check_errors(&inner)?;
        tracing::info!(source = %inner.label, "script hooks loaded");
        Ok(ScriptHooks {
            inner,
            _watcher: None,
        })
    }

    /// Bindet ein Komponenten-Skript an eine Entitaetsmenge. Die Funktionen
    /// `before_create`/`before_patch` des Komponenten-Skripts laufen vor dem
    /// Entitaetsskript und bekommen `(ctx, config)`. Das Skript muss existieren.
    pub fn bind_component(
        &self,
        set: &str,
        script: &str,
        config: Value,
    ) -> Result<(), ScriptError> {
        if !self.inner.components.read().unwrap().contains_key(script) {
            return Err(ScriptError::Script(format!(
                "komponenten-skript '{script}' fehlt in '{}'",
                self.inner.label
            )));
        }
        self.inner
            .bindings
            .write()
            .unwrap()
            .entry(set.to_string())
            .or_default()
            .push(Binding {
                script: script.to_string(),
                config,
            });
        Ok(())
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
            "dir": self.inner.label,
            "sets": sets,
            "errors": *self.inner.errors.read().unwrap(),
            "reloads": self.inner.reloads.load(Ordering::Relaxed),
        })
    }

    /// Schreiboperation vor dem Store (`before_<op>`).
    pub fn before(&self, set: &str, op: &str, payload: &mut Value) -> Result<(), ScriptError> {
        self.before_write(set, op, payload, None)
    }

    /// Wie [`ScriptHooks::before`], zusaetzlich mit dem aktuellen Datensatz (`ctx.current`),
    /// damit ein Patch fehlende Gegenwerte (z. B. den Startzeitpunkt) ergaenzen kann.
    pub fn before_write(
        &self,
        set: &str,
        op: &str,
        payload: &mut Value,
        current: Option<&Value>,
    ) -> Result<(), ScriptError> {
        info!("{} before {}: {:?}", set, op, payload);
        let fname = format!("before_{op}");
        // Gebundene Komponenten zuerst (Regeln je Verwendung), dann das Entitaetsskript.
        for (script, config) in self.bound(set) {
            let ctx = json!({"set": set, "op": op, "kind": "write", "key": null,
                             "payload": payload, "current": current.cloned().unwrap_or(Value::Null)});
            if let Some(res) = self.call_component(&script, &fname, ctx, &config)? {
                take(&res, "payload", payload);
            }
        }
        let ctx = json!({"set": set, "op": op, "kind": "write", "key": null,
                         "payload": payload, "current": current.cloned().unwrap_or(Value::Null)});
        if let Some(res) = self.call(set, &fname, ctx)? {
            take(&res, "payload", payload);
            info!("-> result payload: {:?}", payload)
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

    /// Gebundene Komponenten eines Sets als (Skriptschluessel, Konfiguration).
    fn bound(&self, set: &str) -> Vec<(String, Value)> {
        self.inner
            .bindings
            .read()
            .unwrap()
            .get(set)
            .map(|v| {
                v.iter()
                    .map(|b| (b.script.clone(), b.config.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Ruft `fname` im Komponenten-Skript auf: `(ctx, config)`. `None`, wenn es die Funktion nicht gibt.
    fn call_component(
        &self,
        script: &str,
        fname: &str,
        ctx: Value,
        config: &Value,
    ) -> Result<Option<Value>, ScriptError> {
        let Some(compiled) = self.inner.components.read().unwrap().get(script).cloned() else {
            return Ok(None);
        };
        if !compiled.functions.contains(fname) {
            return Ok(None);
        }
        let arg = rhai::serde::to_dynamic(&ctx)
            .map_err(|e| ScriptError::Script(format!("{script}.{fname}: {e}")))?;
        let cfg = rhai::serde::to_dynamic(config)
            .map_err(|e| ScriptError::Script(format!("{script}.{fname}: {e}")))?;
        DEADLINE.with(|d| d.set(Some(Instant::now() + CALL_TIMEOUT)));
        let result = self.inner.engine.call_fn::<Dynamic>(
            &mut Scope::new(),
            &compiled.ast,
            fname,
            (arg, cfg),
        );
        DEADLINE.with(|d| d.set(None));
        let dynamic = result
            .map_err(|e| ScriptError::Script(format!("{script}.{fname}: {}", describe(&e))))?;
        let value: Value = rhai::serde::from_dynamic(&dynamic)
            .map_err(|e| ScriptError::Script(format!("{script}.{fname}: {e}")))?;
        if !value.is_object() {
            return Err(ScriptError::Script(format!(
                "{script}.{fname}: die Funktion muss das ctx-Objekt zurueckgeben"
            )));
        }
        Ok(Some(value))
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
        let result =
            self.inner
                .engine
                .call_fn::<Dynamic>(&mut Scope::new(), &script.ast, fname, (arg,));
        DEADLINE.with(|d| d.set(None));
        let dynamic =
            result.map_err(|e| ScriptError::Script(format!("{set}.{fname}: {}", describe(&e))))?;
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

fn check_errors(inner: &Inner) -> anyhow::Result<()> {
    let errors = inner.errors.read().unwrap().clone();
    if !errors.is_empty() {
        let list: Vec<String> = errors.iter().map(|(f, e)| format!("{f}: {e}")).collect();
        anyhow::bail!("script compile errors: {}", list.join("; "));
    }
    Ok(())
}

impl Inner {
    fn new(repo: Arc<dyn FileRepository>, label: String) -> Inner {
        Inner {
            engine: build_engine(),
            repo,
            label,
            scripts: RwLock::new(HashMap::new()),
            components: RwLock::new(HashMap::new()),
            bindings: RwLock::new(HashMap::new()),
            errors: RwLock::new(BTreeMap::new()),
            reloads: AtomicU64::new(0),
        }
    }

    /// Liest alle `*.rhai`-Dateien. Ein fehlerhaftes Skript behaelt seine bisherige Version.
    fn reload(&self) {
        let mut next: HashMap<String, Arc<Script>> = HashMap::new();
        let mut errors = BTreeMap::new();
        let old = self.scripts.read().unwrap().clone();

        match self.repo.list("") {
            Ok(names) => {
                for name in names {
                    let Some(set) = rhai_stem(&name) else {
                        continue;
                    };
                    match self.compile(&name) {
                        Ok(script) => {
                            next.insert(set.to_string(), Arc::new(script));
                        }
                        Err(msg) => {
                            errors.insert(name.clone(), msg.clone());
                            tracing::error!(file = %name, "script not reloaded: {msg}");
                            if let Some(prev) = old.get(set) {
                                next.insert(set.to_string(), prev.clone());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                errors.insert(self.label.clone(), e.to_string());
            }
        }

        let mut next_components: HashMap<String, Arc<Script>> = HashMap::new();
        if let Ok(names) = self.repo.list("components") {
            let old_components = self.components.read().unwrap().clone();
            for name in names {
                let Some(stem) = rhai_stem(&name) else {
                    continue;
                };
                let key = format!("components/{stem}");
                let rel = format!("components/{name}");
                match self.compile(&rel) {
                    Ok(script) => {
                        next_components.insert(key, Arc::new(script));
                    }
                    Err(msg) => {
                        errors.insert(rel.clone(), msg.clone());
                        tracing::error!(file = %rel, "component not reloaded: {msg}");
                        if let Some(prev) = old_components.get(&key) {
                            next_components.insert(key, prev.clone());
                        }
                    }
                }
            }
        }

        *self.scripts.write().unwrap() = next;
        *self.components.write().unwrap() = next_components;
        *self.errors.write().unwrap() = errors;
        self.reloads.fetch_add(1, Ordering::Relaxed);
    }

    /// Kompiliert die Datei `rel` aus der Quelle.
    fn compile(&self, rel: &str) -> Result<Script, String> {
        let bytes = self
            .repo
            .read(rel)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Datei nicht gefunden".to_string())?;
        let src = String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())?;
        let ast = self.engine.compile(&src).map_err(|e| e.to_string())?;
        let functions = ast
            .iter_functions()
            .map(|f| f.name.to_string())
            .collect::<HashSet<_>>();
        Ok(Script { ast, functions })
    }
}

/// Stamm eines `*.rhai`-Dateinamens; `None` fuer andere Dateien.
fn rhai_stem(name: &str) -> Option<&str> {
    name.strip_suffix(".rhai")
        .filter(|s| !s.is_empty() && !s.contains('/'))
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

fn rhai_err(msg: &str) -> Box<rhai::EvalAltResult> {
    msg.to_string().into()
}

fn parse_ts(ts: &str) -> Result<chrono::DateTime<chrono::FixedOffset>, Box<rhai::EvalAltResult>> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map_err(|e| rhai_err(&format!("Ungueltiger Zeitpunkt '{ts}': {e}")))
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
    // Datumsfunktionen fuer Skripte. Zeitpunkte sind ISO-8601-Texte mit Offset
    // (z. B. `2026-10-31T09:00:00+01:00`); Ergebnisse behalten den Offset der Eingabe.
    engine.register_fn(
        "ts_add_minutes",
        |ts: &str, minutes: i64| -> Result<String, Box<rhai::EvalAltResult>> {
            let t = parse_ts(ts)?;
            let d = chrono::Duration::try_minutes(minutes)
                .ok_or_else(|| rhai_err("ts_add_minutes: Dauer ungueltig"))?;
            let r = t
                .checked_add_signed(d)
                .ok_or_else(|| rhai_err("ts_add_minutes: Ueberlauf"))?;
            Ok(r.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
        },
    );
    // Differenz `b - a` in ganzen Minuten.
    engine.register_fn(
        "ts_diff_minutes",
        |a: &str, b: &str| -> Result<i64, Box<rhai::EvalAltResult>> {
            let (a, b) = (parse_ts(a)?, parse_ts(b)?);
            Ok((b - a).num_minutes())
        },
    );
    // Rundet auf das naechste Vielfache von `step` Minuten (Sekunden werden auf 0 gesetzt).
    engine.register_fn(
        "ts_round_minutes",
        |ts: &str, step: i64| -> Result<String, Box<rhai::EvalAltResult>> {
            if step <= 0 {
                return Err(rhai_err("ts_round_minutes: Schrittweite muss > 0 sein"));
            }
            // Rundung auf der Wanduhrzeit (lokaler Zeitpunkt), Sekunden und Bruchteile werden verworfen.
            let t = parse_ts(ts)?;
            let local_secs = t.naive_local().and_utc().timestamp();
            let step_s = step * 60;
            let rounded = (local_secs + step_s / 2).div_euclid(step_s) * step_s;
            let naive = chrono::DateTime::from_timestamp(rounded, 0)
                .ok_or_else(|| rhai_err("ts_round_minutes: Ueberlauf"))?
                .naive_utc();
            let r = t
                .offset()
                .from_local_datetime(&naive)
                .earliest()
                .ok_or_else(|| rhai_err("ts_round_minutes: Zeitpunkt nicht eindeutig"))?;
            Ok(r.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
        },
    );
    engine.register_fn("ts_valid", |ts: &str| parse_ts(ts).is_ok());
}

#[cfg(test)]
mod tests;
