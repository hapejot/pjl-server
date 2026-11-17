# odata-yaml-store

Prototyp eines `DataStore`-Backends für OData-Entity-Sets, das jeden
Record als einzelne YAML-Datei ablegt. Gedacht als Vorstufe für eine
spätere Postgres-Implementierung: Der `DataStore`-Trait bleibt identisch,
nur die konkrete Implementierung (`YamlStore`) wird ausgetauscht.

## Voraussetzungen

- Rust; das Crate verwendet Edition 2021 (siehe `Cargo.toml`).

**Hinweis zu Abhängigkeitsversionen:** `Cargo.toml` pinnt `indexmap`,
`hashbrown` und `tempfile` auf konkrete Versionen. Vor einer Änderung dieser
Pins die Toolchain-Kompatibilität und `Cargo.lock` prüfen.

## Struktur

```
src/
  lib.rs          Re-Exports aus data-store und YamlStore
  yaml_store.rs   Datei-basierte Implementierung; Navigation und Draft-Bäume
  tests.rs        Unit- und Integrationsnahe Tests
tests/
  basic.rs        CRUD- und Basisverhalten
  conformance.rs  DataStore-Conformance-Tests
examples/
  demo.rs         Lauffähiges Beispiel (CRUD, Filter, Expand, Draft-Workflow)
```

`DataRecord`, `DataStore`, Draft-Typen, `EntityKey`, `Navigation` und
`StoreError` stammen aus dem Workspace-Crate `data-store` und werden von
`odata-yaml-store` re-exportiert. Der Query-AST (`ODataQuery`, Filter und
Expand) kommt aus `odata::query`; die gemeinsame Filter-, Sortier- und
Paging-Auswertung liegt ebenfalls in `data-store`, nicht in diesem Crate.

## Verwenden

```bash
cargo build
cargo test
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
IsActiveEntity: true
HasActiveEntity: true
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
- **`$apply` (Aggregation/GroupBy, OData V4 Analytics)** ist nicht
  Teil des aktuellen `ODataQuery`-Umfangs und wird vom Store daher nicht
  ausgeführt.
- **`draft_prepare`:** Reines Platzhalterverhalten (gibt aktuellen
  Draft/Active-Stand zurück). Hier wäre der Ansatzpunkt für echte
  Validierungen/Determinations analog zu RAP.
- **Kein Parser für rohe OData-URI-Strings.** `ODataQuery` muss von
  aufrufendem Code (z.B. deinem bestehenden URI-Parsing) befüllt werden.
- **Navigationen:** `$expand` für Sammlungsnavigationen liefert alle Treffer.
  Eine einwertige Navigation liefert `null` bei keinem Treffer und den
  Datensatz bei genau einem Treffer; mehrere Treffer führen zu einem
  `Conflict`-Fehler.
- **Kompositions-Drafts:** Edit und Activate propagieren rekursiv über die
  registrierten Kompositions-Kinder. Die Aktualisierung eines ganzen Baums
  ist jedoch nicht transaktional; ein Fehler kann nach bereits geschriebenen
  Kinddatensätzen auftreten.

## Migration nach Postgres

Der Trait `DataStore` (im Crate `data-store`) ist bewusst so geschnitten,
dass er 1:1 gegen eine `sqlx`- oder `diesel`-basierte Implementierung
austauschbar ist, ohne Aufrufer-Code anzufassen:

```rust
pub struct PostgresStore { /* Pool, etc. */ }

impl DataStore for PostgresStore {
    // gleiche Methoden, SQL statt YAML-Dateien
}
```

`EntityKey` und `ODataQuery` bleiben dabei wiederverwendbar. Ein
relationales Backend kann die gemeinsame Filterauswertung in
`data-store` durch eine SQL-Übersetzung ergänzen oder ersetzen.
