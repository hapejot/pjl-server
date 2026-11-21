use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, RwLock},
};

use include_dir::{include_dir, Dir};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use st::{Interpreter, InterpreterError, memory::Value};

static RESOURCES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/resources");

#[derive(Debug, thiserror::Error)]
pub enum SmalltalkError {
    #[error("{0}")]
    Script(String),
}

const BRIDGE_SOURCE: &str = r#"
Object subclass: PjlJsonObject [
    | keys values count |

    initialize [
        keys := Array new: 1000.
        values := Array new: 1000.
        count := 0.
        ^self
    ]

    at: key [
        | i |
        i := 1.
        [i <= count] whileTrue: [
            ((keys at: i) = key) ifTrue: [^values at: i].
            i := i + 1
        ].
        ^nil
    ]

    at: key put: value [
        | i |
        i := 1.
        [i <= count] whileTrue: [
            ((keys at: i) = key) ifTrue: [
                values at: i put: value.
                ^value
            ].
            i := i + 1
        ].
        (count >= 1000) ifTrue: [self error: 'PjlJsonObject: maximale Feldanzahl ueberschritten'].
        count := count + 1.
        keys at: count put: key.
        values at: count put: value.
        ^value
    ]

    keys [ ^keys ]
    values [ ^values ]
    count [ ^count ]
]

Object subclass: PjlContext [
    | set op kind key property payload current query record patch |
    set [ ^set ]
    set: value [ set := value. ^set ]
    op [ ^op ]
    op: value [ op := value. ^op ]
    kind [ ^kind ]
    kind: value [ kind := value. ^kind ]
    key [ ^key ]
    key: value [ key := value. ^key ]
    property [ ^property ]
    property: value [ property := value. ^property ]
    payload [ ^payload ]
    payload: value [ payload := value. ^payload ]
    current [ ^current ]
    current: value [ current := value. ^current ]
    query [ ^query ]
    query: value [ query := value. ^query ]
    record [ ^record ]
    record: value [ record := value. ^record ]
    patch [ ^patch ]
    patch: value [ patch := value. ^patch ]
]

Object subclass: PjlEntity [
    beforeCreate: ctx [ ^ctx ]
    beforePatch: ctx [ ^ctx ]
    beforeDelete: ctx [ ^ctx ]
    beforeDraftEdit: ctx [ ^ctx ]
    beforeDraftActivate: ctx [ ^ctx ]
    beforeDraftPrepare: ctx [ ^ctx ]
    afterCreate: ctx [ ^ctx ]
    afterPatch: ctx [ ^ctx ]
    afterDelete: ctx [ ^ctx ]
    afterDraftEdit: ctx [ ^ctx ]
    afterDraftActivate: ctx [ ^ctx ]
    afterDraftPrepare: ctx [ ^ctx ]
    beforeRead: ctx [ ^ctx ]
    afterRead: ctx [ ^ctx ]
]
"#;

static RESOURCE_ROOT: OnceLock<PathBuf> = OnceLock::new();

fn resource_root() -> std::io::Result<&'static Path> {
    if let Some(path) = RESOURCE_ROOT.get() {
        return Ok(path);
    }
    let path = std::env::temp_dir().join(format!(
        "pjl-server-smalltalk-{}",
        std::process::id()
    ));
    if !path.exists() {
        std::fs::create_dir_all(&path)?;
    }
    RESOURCES.extract(&path)?;
    let root = RESOURCE_ROOT.get_or_init(|| path);
    Ok(root)
}

fn build_interpreter() -> Result<Interpreter, InterpreterError> {
    let root = resource_root().map_err(|e| InterpreterError::Bootstrap {
        path: PathBuf::from("<smalltalk resources>"),
        source: Box::new(e),
    })?;
    Interpreter::with_boot_file(root.join("boot.yaml"), root.join("smalltalk"))
}

fn stem(name: &str) -> Option<&str> {
    name.strip_suffix(".st").filter(|s| !s.is_empty())
}

struct Inner {
    interpreter: RwLock<Arc<Interpreter>>,
    scripts: RwLock<BTreeMap<String, String>>,
    errors: RwLock<BTreeMap<String, String>>,
    dir: RwLock<PathBuf>,
}

pub struct SmalltalkHooks {
    inner: Arc<Inner>,
    _watcher: Option<RecommendedWatcher>,
}

impl SmalltalkHooks {
    pub fn load(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let inner = Arc::new(Inner {
            interpreter: RwLock::new(Arc::new(build_interpreter()?)),
            scripts: RwLock::new(BTreeMap::new()),
            errors: RwLock::new(BTreeMap::new()),
            dir: RwLock::new(dir.clone()),
        });
        inner.reload()?;
        let watcher_inner = inner.clone();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            if let Ok(event) = result {
                if script_change(&event.kind)
                    && event.paths.iter().any(|path| path.extension().is_some_and(|ext| ext == "st"))
                {
                    let _ = tx.send(());
                }
            }
        })?;
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(std::time::Duration::from_millis(300)).is_ok() {}
                let _ = watcher_inner.reload();
            }
        });
        Ok(Self {
            inner,
            _watcher: Some(watcher),
        })
    }

    pub fn reload(&self) -> anyhow::Result<usize> {
        self.inner.reload()?;
        Ok(self.inner.errors.read().unwrap().len())
    }

    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "sets": self.inner.scripts.read().unwrap().keys().collect::<Vec<_>>(),
            "errors": self.inner.errors.read().unwrap().clone(),
        })
    }

    pub fn has_action(&self, set: &str, name: &str) -> bool {
        self.inner.scripts.read().unwrap().contains_key(set)
            && self.inner
                .scripts
                .read()
                .unwrap()
                .get(set)
                .is_some_and(|source| source.contains(&format!("action_{name}:")))
    }

    pub fn before(&self, set: &str, op: &str, payload: &mut serde_json::Value) -> Result<(), SmalltalkError> {
        self.before_operation(set, op, payload, None, None)
    }

    pub fn before_operation(
        &self,
        set: &str,
        op: &str,
        payload: &mut serde_json::Value,
        key: Option<serde_json::Value>,
        current: Option<&serde_json::Value>,
    ) -> Result<(), SmalltalkError> {
        if !self.inner.scripts.read().unwrap().contains_key(set) {
            return Ok(());
        }
        let mut context = ContextBuilder::new(set, op, "write");
        context.payload(payload);
        context.key(key.as_ref());
        context.current(current);
        let result = self.call(set, &format!("before{}:", pascal(op)), &context)?;
        take_payload(result, payload)
    }

    pub fn after(&self, set: &str, op: &str, payload: &mut serde_json::Value) -> Result<(), SmalltalkError> {
        if !self.inner.scripts.read().unwrap().contains_key(set) {
            return Ok(());
        }
        let mut context = ContextBuilder::new(set, op, "write");
        context.payload(payload);
        let result = self.call(set, &format!("after{}:", pascal(op)), &context)?;
        take_payload(result, payload)
    }

    pub fn before_read(
        &self,
        set: &str,
        key: Option<serde_json::Value>,
        query: &mut serde_json::Value,
    ) -> Result<(), SmalltalkError> {
        if !self.inner.scripts.read().unwrap().contains_key(set) {
            return Ok(());
        }
        let mut context = ContextBuilder::new(set, "read", if key.is_some() { "entity" } else { "list" });
        context.key(key.as_ref());
        context.query(query);
        let result = self.call(set, "beforeRead:", &context)?;
        take_query(result, query)
    }

    pub fn after_read(
        &self,
        set: &str,
        kind: &str,
        key: Option<serde_json::Value>,
        property: Option<&str>,
        payload: &mut serde_json::Value,
    ) -> Result<(), SmalltalkError> {
        if !self.inner.scripts.read().unwrap().contains_key(set) {
            return Ok(());
        }
        let mut context = ContextBuilder::new(set, "read", kind);
        context.key(key.as_ref());
        context.property(property);
        context.payload(payload);
        let result = self.call(set, "afterRead:", &context)?;
        take_payload(result, payload)
    }

    pub fn action(
        &self,
        set: &str,
        name: &str,
        record: serde_json::Value,
    ) -> Result<serde_json::Value, SmalltalkError> {
        if !self.inner.scripts.read().unwrap().contains_key(set) {
            return Err(SmalltalkError::Script(format!("{set}: Skript fehlt")));
        }
        let mut context = ContextBuilder::new(set, "action", "entity");
        context.record(&record);
        let result = self.call(set, &format!("action_{name}:"), &context)?;
        match context_object_value(result, "patch") {
            Some(patch) if patch.is_object() && !patch.as_object().is_some_and(|v| v.is_empty()) => Ok(patch),
            _ => Err(SmalltalkError::Script(format!(
                "{set}.action_{name}: ctx.patch muss ein nichtleeres Objekt sein"
            ))),
        }
    }

    fn call(
        &self,
        set: &str,
        method: &str,
        context: &ContextBuilder,
    ) -> Result<Value, SmalltalkError> {
        let class = set;
        let (declarations, statements) = context.build();
        let mut source = format!("| ctx payload {} | ctx := PjlContext basicNew. ", declarations.join(" "));
        source.push_str("payload := nil. ");
        source.push_str(&statements);
        source.push_str(&format!(
            " ctx payload: payload. ^({class} basicNew) {method} ctx"
        ));
        self.inner
            .interpreter
            .read()
            .unwrap()
            .eval(&source)
            .map_err(|e| SmalltalkError::Script(format!("{set}.{method}: {e}")))
    }
}

struct ContextBuilder {
    set: String,
    op: String,
    kind: String,
    key: Option<serde_json::Value>,
    property: Option<String>,
    payload: Option<serde_json::Value>,
    current: Option<serde_json::Value>,
    query: Option<serde_json::Value>,
    record: Option<serde_json::Value>,
}

impl ContextBuilder {
    fn new(set: &str, op: &str, kind: &str) -> Self {
        Self {
            set: set.to_string(),
            op: op.to_string(),
            kind: kind.to_string(),
            key: None,
            property: None,
            payload: None,
            current: None,
            query: None,
            record: None,
        }
    }

    fn key(&mut self, value: Option<&serde_json::Value>) {
        self.key = value.cloned();
    }

    fn property(&mut self, value: Option<&str>) {
        self.property = value.map(ToString::to_string);
    }

    fn payload(&mut self, value: &serde_json::Value) {
        self.payload = Some(value.clone());
    }

    fn current(&mut self, value: Option<&serde_json::Value>) {
        self.current = value.cloned();
    }

    fn query(&mut self, value: &serde_json::Value) {
        self.query = Some(value.clone());
    }

    fn record(&mut self, value: &serde_json::Value) {
        self.record = Some(value.clone());
    }

    fn build(&self) -> (Vec<String>, String) {
        let mut declarations = Vec::new();
        let mut statements = String::new();
        let mut counter = 0usize;

        statements.push_str(&format!(
            "ctx set: {}. ctx op: {}. ctx kind: {}. ",
            smalltalk_primitive(&self.set),
            smalltalk_primitive(&self.op),
            smalltalk_primitive(&self.kind),
        ));

        if let Some(property) = &self.property {
            statements.push_str(&format!("ctx property: {}. ", smalltalk_primitive(property)));
        } else {
            statements.push_str("ctx property: nil. ");
        }

        let fields = [
            ("key", &self.key),
            ("payload", &self.payload),
            ("current", &self.current),
            ("query", &self.query),
            ("record", &self.record),
        ];
        for (name, value) in fields {
            if let Some(value) = value {
                let (vars, source_statements) =
                    json_to_smalltalk(value, &mut counter);
                declarations.extend(vars.keys.clone());
                statements.push_str(&source_statements);
                let result = vars.result.clone().unwrap_or_else(|| "nil".to_string());
                if name == "payload" {
                    statements.push_str(&format!(" payload := {result}. ctx payload: payload. "));
                } else {
                    statements.push_str(&format!(
                        "ctx {name}: {}. ",
                        result
                    ));
                }
            } else if name == "payload" {
                statements.push_str("payload := nil. ctx payload: payload. ");
            } else {
                statements.push_str(&format!("ctx {name}: nil. "));
            }
        }
        (declarations, statements)
    }
}

#[derive(Default)]
struct Vars {
    keys: Vec<String>,
    result: Option<String>,
}

fn json_to_smalltalk(value: &serde_json::Value, counter: &mut usize) -> (Vars, String) {
    let mut vars = Vars::default();
    let (expr, statements) = json_expr(value, counter, &mut vars);
    vars.result = Some(expr);
    (vars, statements)
}

fn json_expr(
    value: &serde_json::Value,
    counter: &mut usize,
    vars: &mut Vars,
) -> (String, String) {
    match value {
        serde_json::Value::Null => ("nil".to_string(), String::new()),
        serde_json::Value::Bool(value) => (value.to_string(), String::new()),
        serde_json::Value::Number(value) => (value.to_string(), String::new()),
        serde_json::Value::String(value) => (smalltalk_primitive(value), String::new()),
        serde_json::Value::Array(items) => {
            let temp = next_temp(counter, vars);
            let mut elements = Vec::new();
            let mut statements = String::new();
            for item in items {
                let (expr, source) = json_expr(item, counter, vars);
                statements.push_str(&source);
                elements.push(expr);
            }
            statements.push_str(&format!(
                "{} := {{ {} }}. ",
                temp,
                elements.join(". ")
            ));
            (temp, statements)
        }
        serde_json::Value::Object(map) => {
            let temp = next_temp(counter, vars);
            let mut statements = format!("{temp} := PjlJsonObject basicNew. {temp} initialize. ");
            for (key, value) in map {
                let (expr, source) = json_expr(value, counter, vars);
                statements.push_str(&source);
                statements.push_str(&format!(
                    "{temp} at: {} put: {}. ",
                    smalltalk_primitive(key),
                    expr
                ));
            }
            (temp, statements)
        }
    }
}

fn next_temp(counter: &mut usize, vars: &mut Vars) -> String {
    let name = format!("v{}", *counter);
    *counter += 1;
    vars.keys.push(name.clone());
    name
}

fn smalltalk_primitive(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn script_change(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

fn pascal(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect()
}

fn take_payload(result: Value, target: &mut serde_json::Value) -> Result<(), SmalltalkError> {
    let Some(payload) = context_object_value(result, "payload") else {
        return Err(SmalltalkError::Script(
            "Smalltalk-Hook muss ein PjlContext-Objekt zurueckgeben".into(),
        ));
    };
    *target = payload;
    Ok(())
}

fn take_query(result: Value, target: &mut serde_json::Value) -> Result<(), SmalltalkError> {
    let Some(query) = context_object_value(result, "query") else {
        return Err(SmalltalkError::Script(
            "Smalltalk-Hook muss ein PjlContext-Objekt zurueckgeben".into(),
        ));
    };
    *target = query;
    Ok(())
}

fn context_object_value(result: Value, field: &str) -> Option<serde_json::Value> {
    let Value::Object(object) = result else {
        return None;
    };
    let class = object.class().ok()?;
    let name = class.name().ok()?;
    if name != "PjlContext" {
        return None;
    }
    let vars = object.get_instance_vars().ok()?;
    let index = match field {
        "payload" => 5,
        "query" => 7,
        "patch" => 9,
        _ => return None,
    };
    vars.get(index).and_then(|value| smalltalk_value_to_json(value).ok())
}

fn smalltalk_value_to_json(value: &Value) -> Result<serde_json::Value, SmalltalkError> {
    match value {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::Boolean(value) => Ok(serde_json::Value::Bool(*value)),
        Value::Integer(value) => Ok(serde_json::json!(value)),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| SmalltalkError::Script("Ungueltige Floating-Point-Zahl".into())),
        Value::String(value) => value
            .to_rust_string()
            .map(serde_json::Value::String)
            .map_err(|error| SmalltalkError::Script(error)),
        Value::Symbol(value) => Ok(serde_json::Value::String(value.to_string())),
        Value::Array(items) => {
            let items = items
                .lock()
                .map_err(|error| SmalltalkError::Script(error.to_string()))?;
            let mut result = Vec::with_capacity(items.len());
            for item in items.iter() {
                result.push(smalltalk_value_to_json(item)?);
            }
            Ok(serde_json::Value::Array(result))
        }
        Value::Object(object) => {
            let class = object
                .class()
                .map_err(SmalltalkError::Script)?;
            if class.name().map_err(SmalltalkError::Script)? != "PjlJsonObject" {
                return Err(SmalltalkError::Script(
                    "Unerwartetes Smalltalk-Objekt in JSON-Kontext".into(),
                ));
            }
            let vars = object
                .get_instance_vars()
                .map_err(SmalltalkError::Script)?;
            let (Some(keys), Some(values), Some(Value::Integer(count))) =
                (vars.get(0), vars.get(1), vars.get(2))
            else {
                return Err(SmalltalkError::Script(
                    "PjlJsonObject hat ein ungueltiges Layout".into(),
                ));
            };
            let Value::Array(keys) = keys else {
                return Err(SmalltalkError::Script(
                    "PjlJsonObject.keys muss ein Array sein".into(),
                ));
            };
            let Value::Array(values) = values else {
                return Err(SmalltalkError::Script(
                    "PjlJsonObject.values muss ein Array sein".into(),
                ));
            };
            let keys = keys
                .lock()
                .map_err(|error| SmalltalkError::Script(error.to_string()))?;
            let values = values
                .lock()
                .map_err(|error| SmalltalkError::Script(error.to_string()))?;
            let mut map = serde_json::Map::new();
            for index in 0..usize::try_from(*count).unwrap_or(0).min(keys.len()) {
                let key = match &keys[index] {
                    Value::String(value) => value
                        .to_rust_string()
                        .map_err(SmalltalkError::Script)?,
                    _ => return Err(SmalltalkError::Script(
                        "PjlJsonObject-Schluessel muss ein String sein".into(),
                    )),
                };
                map.insert(key, smalltalk_value_to_json(&values[index])?);
            }
            Ok(serde_json::Value::Object(map))
        }
        other => Err(SmalltalkError::Script(format!(
            "Smalltalk-Wert kann nicht nach JSON konvertiert werden: {other:?}"
        ))),
    }
}

impl Inner {
    fn reload(&self) -> anyhow::Result<()> {
        let dir = self.dir.read().unwrap().clone();
        let names = std::fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with(".st"))
            .collect::<BTreeSet<_>>();

        let mut sources = BTreeMap::new();
        let mut errors = BTreeMap::new();
        for name in names {
            let set = stem(&name).unwrap_or(&name);
            match std::fs::read_to_string(dir.join(&name)) {
                Ok(source) => {
                    sources.insert(set.to_string(), source);
                }
                Err(error) => {
                    errors.insert(name, error.to_string());
                }
            }
        }

        let interpreter = build_interpreter().map_err(|e| anyhow::anyhow!(e))?;
        interpreter.load_source(BRIDGE_SOURCE).map_err(|e| anyhow::anyhow!(e))?;
        for (set, source) in &sources {
            if let Err(error) = interpreter.load_source(source) {
                errors.insert(format!("{set}.st"), error.to_string());
            }
        }
        if errors.is_empty() {
            *self.scripts.write().unwrap() = sources;
            self.errors.write().unwrap().clear();
            // The interpreter is replaced only after every script loads successfully.
            *self.interpreter.write().unwrap() = Arc::new(interpreter);
        } else {
            *self.errors.write().unwrap() = errors;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn smalltalk_hook_mutates_payload_and_handles_action() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("AccountSet.st"),
            r#"
PjlEntity subclass: AccountSet [
    beforeCreate: ctx [
        ctx payload at: 'Name' put: 'Firmenname'.
        ^ctx
    ]
    action_starten: ctx [
        ctx patch: (PjlJsonObject basicNew).
        ctx patch initialize.
        ctx patch at: 'status' put: 'In Bearbeitung'.
        ^ctx
    ]
]
"#,
        )
        .unwrap();

        let hooks = SmalltalkHooks::load(dir.path()).unwrap();
        let mut payload = serde_json::json!({"Name": "abc"});
        hooks.before("AccountSet", "create", &mut payload).unwrap();
        assert_eq!(payload["Name"], "Firmenname");

        let record = serde_json::json!({"status": "Geplant"});
        let patch = hooks.action("AccountSet", "starten", record).unwrap();
        assert_eq!(patch["status"], "In Bearbeitung");
    }

    #[test]
    fn missing_script_is_no_hook() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = SmalltalkHooks::load(dir.path()).unwrap();
        let mut payload = serde_json::json!({"a": 1});
        hooks.before("OtherSet", "create", &mut payload).unwrap();
        assert_eq!(payload, serde_json::json!({"a": 1}));
    }

    #[test]
    fn watcher_ignores_read_and_access_events() {
        assert!(!script_change(&EventKind::Access(
            notify::event::AccessKind::Open(notify::event::AccessMode::Read)
        )));
        assert!(script_change(&EventKind::Modify(
            notify::event::ModifyKind::Data(notify::event::DataChange::Any)
        )));
    }

    #[test]
    fn poc_crm_account_rules_load_and_validate_draft() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../poc-crm/scripts");
        let hooks = SmalltalkHooks::load(dir).unwrap();

        let mut payload = serde_json::json!({"name": "Alt"});
        hooks.before("AccountSet", "create", &mut payload).unwrap();
        assert_eq!(payload["name"], "Firmenname");

        let draft = serde_json::json!({"city": "Berlin"});
        let current = serde_json::json!({"draft": draft});
        let mut payload = serde_json::json!({});
        let error = hooks
            .before_operation(
                "AccountSet",
                "draft_prepare",
                &mut payload,
                None,
                Some(&current),
            )
            .unwrap_err();
        assert!(error.to_string().contains("Stadt ist nicht Hannover"));
        let activation_error = hooks
            .before_operation(
                "AccountSet",
                "draft_activate",
                &mut payload,
                None,
                Some(&current),
            )
            .unwrap_err();
        assert!(activation_error
            .to_string()
            .contains("Stadt ist nicht Hannover"));

        let draft = serde_json::json!({"city": "Hannover"});
        let current = serde_json::json!({"draft": draft});
        hooks
            .before_operation(
                "AccountSet",
                "draft_prepare",
                &mut payload,
                None,
                Some(&current),
            )
            .unwrap();
    }
}
