# odata-yaml-store

Prototyp eines `DataStore`-Backends für OData-Entity-Sets, das jeden
Record als einzelne YAML-Datei ablegt. Gedacht als Vorstufe für eine
spätere Postgres-Implementierung: Der `DataStore`-Trait bleibt identisch,
nur die konkrete Implementierung (`YamlStore`) wird ausgetauscht.

## Voraussetzungen

- Rust (getestet mit 1.75, sollte mit neueren Versionen ebenfalls laufen)

**Hinweis zu Abhängigkeitsversionen:** In `Cargo.toml` sind `indexmap`,
`hashbrown` und `tempfile` bewusst auf ältere Versionen gepinnt
(`=2.2.6` / `=0.14.5` / `=3.10.1`). Neuere Versionen dieser Crates
verlangen Rust Edition 2024, was ältere Toolchains (< 1.85 bzw. ohne
aktiviertes Edition-2024-Feature) nicht unterstützen. Falls du eine
aktuelle Rust-Toolchain (via `rustup`) nutzt, kannst du diese Pins
entfernen oder lockern.

## Struktur

```
src/
  error.rs        StoreError
  key.rs          EntityKey, ParentKey (inkl. RAP-Draft-Dimension)
  query.rs        ODataQuery, FilterExpr, ExpandItem, ... (kompletter Query-AST)
  metadata.rs      EntityMetadata: Key-Felder + Navigationen pro Entity-Set
  filter_eval.rs   Auswertung von FilterExpr gegen serde_json::Value
  datastore.rs     DataStore-Trait (+ DataStoreDebugExt)
  yaml_store.rs    YamlStore: Datei-basierte Implementierung von DataStore
  lib.rs           Re-Exports + DraftActionExt + Tests
examples/
  demo.rs          Lauffähiges Beispiel (CRUD, Filter, Expand, Draft-Workflow)
```

## Verwenden

```bash
cargo build
cargo test --features debug-tools
cargo run --example demo
```

## Storage-Layout auf Disk

```
<root>/
  Orders/
    active/
      <hash>.yaml
    draft/
      <hash>.yaml
  OrderItems/
    active/
      <hash>.yaml
```

Jede Datei enthält den vollständigen `EntityKey` (lesbar, nicht nur den
Hash im Dateinamen) sowie die eigentlichen Daten:

```yaml
key:
  fields:
    ID: "1"
  is_active_entity: true
data:
  ID: "1"
  CustomerName: "Müller GmbH"
  Status: "Released"
```

Der Dateiname selbst ist ein Hash des Keys (`storage_id()`), weil
zusammengesetzte Keys oder GUIDs Zeichen enthalten können, die als
Dateiname problematisch wären.

## Setup-Beispiel

```rust
use odata_yaml_store::{EntityMetadata, NavigationDef, YamlStore};

let mut metadata = EntityMetadata::new();
metadata.register_key("Orders", &["ID"]);
metadata.register_key("OrderItems", &["ID"]);
metadata.register_navigation(
    "Orders",
    "Items",
    NavigationDef {
        target_set: "OrderItems".to_string(),
        foreign_key_field: "OrderID".to_string(),
    },
);

let store = YamlStore::open("./data", metadata)?;
```

## Bekannte Vereinfachungen (bewusste Prototyp-Entscheidungen)

- **Write-Through statt Write-Behind:** Jede Operation schreibt sofort auf
  Disk. `commit()` ist aktuell ein No-Op. Für einen Prototyp ohne
  Performance-Anspruch ausreichend und weniger fehleranfällig als ein
  Dirty-Tracking über Transaktionsgrenzen.
- **`any`/`all` im Filter:** Das Predicate wird direkt gegen den
  Collection-Item-Wert ausgewertet; `lambda_var` wird aktuell nicht als
  echter Variablenkontext verwaltet. Reicht für den Standardfall
  (`Items/any(i:i/Qty gt 10)`), nicht für verschachtelte `any`/`all` mit
  mehreren unterschiedlichen Lambda-Variablen im selben Ausdruck.
- **`$apply` (Aggregation/GroupBy, OData V4 Analytics)** ist nicht
  abgebildet – falls benötigt, müsste `ApplyTransformation` (siehe
  Konversationsverlauf) ergänzt werden.
- **Composition-Scoping über Parent:** Geht aktuell davon aus, dass der
  Foreign Key genau ein Parent-Key-Feld referenziert. Bei zusammengesetzten
  Parent-Keys mit mehreren FK-Feldern im Kind müsste das erweitert werden.
- **`draft_prepare`:** Reines Platzhalterverhalten (gibt aktuellen
  Draft/Active-Stand zurück). Hier wäre der Ansatzpunkt für echte
  Validierungen/Determinations analog zu RAP.
- **Kein Parser für rohe OData-URI-Strings.** `ODataQuery` muss von
  aufrufendem Code (z.B. deinem bestehenden URI-Parsing) befüllt werden.

## Migration nach Postgres

Der Trait `DataStore` (in `datastore.rs`) ist bewusst so geschnitten,
dass er 1:1 gegen eine `sqlx`- oder `diesel`-basierte Implementierung
austauschbar ist, ohne Aufrufer-Code anzufassen:

```rust
pub struct PostgresStore { /* Pool, etc. */ }

impl DataStore for PostgresStore {
    // gleiche Methoden, SQL statt YAML-Dateien
}
```

`EntityKey`, `ParentKey`, `ODataQuery` und `FilterExpr` bleiben dabei
unverändert wiederverwendbar – die `FilterExpr`-AST müsste dann in eine
`WHERE`-Klausel statt in In-Memory-Vergleiche übersetzt werden
(`filter_eval.rs` durch einen SQL-Query-Builder ersetzen).
