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
  key.rs          EntityKey (inkl. RAP-Draft-Dimension)
  filter_eval.rs  Auswertung von FilterExpr gegen serde_json::Value
  datastore.rs    DataStore-Trait (+ DataStoreDebugExt), DataRecord-Datenmodell, Navigation
  yaml_store.rs   YamlStore: Datei-basierte Implementierung von DataStore
  lib.rs          Re-Exports + DraftDataStore-Trait + Tests
examples/
  demo.rs          Lauffähiges Beispiel (CRUD, Filter, Expand, Draft-Workflow)
```

**Hinweis zum Query-AST:** Der Query-Builder (`ODataQuery`,
`FilterExpr`, `ExpandItem`, `Navigation`, …) wird **nicht** in diesem Crate
lokal definiert — er stammt aus der externen `odata`-Abhängigkeit
(`odata::query`). Die frühere `query.rs` entfällt deshalb; die
Query-Struktur ist vollständig bei `odata::query::*`, womit der
`DataStore`-Trait von der konkreten Query-Implementierung losgekoppelt
ist. `lib.rs` re-exportet öffentlich: `DataStore`, den
`DraftDataStore`-Trait (`draft_edit` / `draft_activate` /
`draft_prepare`), `DataStoreDebugExt`, `EntityKey`, `YamlStore` und
`StoreError`. Die Datentypen `DraftAction`, `DraftAdministrativeData`
und `DraftMessage` gehören in `datastore.rs`.

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
      <storage_id>.yaml
    draft/
      <storage_id>.yaml
  OrderItems/
    active/
      <storage_id>.yaml
    draft/
      <storage_id>.yaml
```

Jede YAML-Datei enthält den vollständigen `DataRecord` — also die Metadaten
(`ID`, `IsActiveEntity`, ggf. `DraftAdministrativeData`, …) und die
Geschäftsfelder (`CustomerName`, `Status`, …) im selben flachen Mapping:

```yaml
ID: "00000000-0000-0000-0000-000000000001"
IsActiveEntity: "true"
HasActiveEntity: "true"
CustomerName: "Müller GmbH"
Status: "Released"
```

Die Systemfelder werden über `serde(rename)` positioniert und über
`#[serde(flatten)]` zusammen mit den Geschäftsfeldern serialisiert. Der
Dateiname selbst ist ein String des `storage_id()` des Keys, weil
zusammengesetzte Keys oder GUIDs Zeichen enthalten können, die als
Dateiname problematisch wären.

## Setup-Beispiel

```rust
use odata_yaml_store::YamlStore;

let store = YamlStore::open("./data")?;
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
- **`draft_prepare`:** Reines Platzhalterverhalten (gibt aktuellen
  Draft/Active-Stand zurück). Hier wäre der Ansatzpunkt für echte
  Validierungen/Determinations analog zu RAP.
- **Kein Parser für rohe OData-URI-Strings.** `ODataQuery` muss von
  aufrufendem Code (z.B. deinem bestehenden URI-Parsing) befüllt werden.
- **Nicht implementierte Stellen (aktuell `todo!()`):**
  - `DataStore::read_sibling_entity` ist **nicht implementiert**.
  - `YamlStore::expand_single_row` liefert für eine **Single-Referenz mit
    mehr als einem Child** (`match subrows { [v] => …, _ => todo!() }`,
    line ~148) `todo!()`. Die **Sammlungsnavigation** hingegen liefert
    stets die volle Trefferliste (`row[from_field] = subrows`) und trifft
    hier nicht auf den Fallback.
  - In `filter_eval.rs` sind alle relationalen Vergleichsoperatoren
    (`Gt`, `Ge`, `Lt`, `Le`) sowie `evaluate_operand` und
    `FunctionCall` (`Expr::FunctionCall`) **`todo!()`**.

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

`EntityKey`, `ODataQuery` und `FilterExpr` bleiben dabei unverändert
wiederverwendbar – die `FilterExpr`-AST müsste dann in eine
`WHERE`-Klausel statt in In-Memory-Vergleiche übersetzt werden
(`filter_eval.rs` durch einen SQL-Query-Builder ersetzen).
