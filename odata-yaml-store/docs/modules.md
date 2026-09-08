# Technische Moduldokumentation: `odata-yaml-store`

> Prototyp eines `DataStore`-Backends für OData-Entity-Sets, der jeden Record
> als einzelne YAML-Datei persistiert. Gedacht als Vorstufe für eine spätere
> Postgres-Implementierung.

## 1. Projektziel & Architekturprinzip

Das Crate (`odata_yaml_store`, v0.1.0) ist eine lose, auf Erweiterbarkeit ausgerichtete Bibliothek für den OData-Datenzugang im
Business-Context-Stil (an SAP-BDxf-RAP angelehnt). Kernidee:

- **Ein Record = eine Datei.** Jeder Business-Obejekt-Satz wird als eigenständige, lesbar serialisierte YAML-Datei auf disk gespeichert.
- **Draft/Active-Modell.** Jedes Geschäftsobjekt existiert in zwei Schichten:
  - *active* (`IsActiveEntity = true`) – der offiziell(committed) Geschäftsobjekt-Satz.
  - *draft* (`IsActiveEntity = false`) – eine eigenständige Arbeitskopie zum Bearbeiten.
- **Storage-Trennung.** Die Persistenz-Implementierung ist bewusst vom `DataStore`-Trait getrennt, damit die Aufrufer-Code (z. B. der OData-Server)Implementation-unabhängig bleibt.

Die `odata`-Abhängigkeit liefert die Query-Struktur (`ODataQuery`, Expr/Filter-AST, Path-Segments). Der Store interpretiert
diese Query-Struktur gegen die in-memory/gelesenen Records. Ein Parser für rohe OData-URI-Strings gehört nicht hierher (sie
werden vom aufrufenden Code in `ODataQuery` gefüllt).

## 2. Verzeichnisstruktur & Module

| Datei            | Zeilen (nahezu) | Kernaufgabe                                                                 |
|------------------|-----------------|-----------------------------------------------------------------------------|
| `lib.rs`         | ~268            | Re-Exports, `DraftDataStore`-Trait, Store-Tests                             |
| `datastore.rs`   | ~343            | Datenmodell (`DataRecord`), `Navigation`, `DataStore`-Trait, Debug-Ext      |
| `yaml_store.rs`  | ~386            | `YamlStore` – konkrete Datei-basierte Implementierung                       |
| `key.rs`         | ~68             | `EntityKey` (Entity-Identifikation inkl. Draft-Dimension)                    |
| `filter_eval.rs` | ~165            | Interpretation von Filter/Compare-Expressions                                 |
| `error.rs`       | ~32             | Fehlermodell `StoreError`                                                   |

> Hinweis: Der README nennt zusätzlich eine `query.rs` (ODataQuery/FilterExpr/ExpandItem). Diese liegt im jeweiligen
> Snapshot **nicht** im `src/` vor – der `ODataQuery`-Typ stammt aus der externen `odata`-Abhängigkeit. Die Doku bezieht
> sich daher auf `odata::query::{ODataQuery, ...}` statt auf lokale Definitionen.

### Verwenden des Pakets

```bash
cargo build
cargo test --features debug-tools   # mod-interne + debug-Erweiterung
cargo run --example demo            # CRUD / Filter / Expand / Draft-Workflow
```

## 3. Disk-Layout

```
<root>/
  Orders/
    active/<storage_id>.yaml     # IsActiveEntity = true
    draft/<storage_id>.yaml      # IsActiveEntity = false   (Bearbeitungsstand)
  OrderItems/
    active/<storage_id>.yaml
    draft/<storage_id>.yaml
```

Jeder Record enthält in der Datei beide Schichten: eine lesbare `key`-Sektion (EntityKey: ID + IsActiveEntity) und die
Datensektion mit den eigentlichen Feldern. Der Dateiname ist **Nicht** der Key selbst, sondern ein String des `storage_id()`
(bzw. des Key-`Display`), weil zusammengesetzte Keys/GUIDs Zeichen enthalten können, die als Dateiname ungeeignet sind.

> Abweichung zum README-Beispiel: Die aktuelle `DataRecord`-Serialisierung nutzt **keinen** eigenen `key:/data:`-Wrapper –
> die Felder (inkl. `ID`, `IsActiveEntity`, `DraftAdministrativeData`, …) und die Geschäftsfelder werden über flachen
> serde-YAML-Mapping (einschl. `serde(flatten)`) gespeichert und über `from_json`/`DataRecord::from_json` interpretiert.

## 4. Module im Detail

### 4.1 `lib.rs`

Verklettert die Submodule und re-exportet die zentralen Public-Typen:
`DataStore`, `DraftDataStore`-Trait, `DataStoreDebugExt`, `EntityKey`, `YamlStore`, `StoreError`, `DraftAction`.

**`DraftDataStore`-Trait** (erweitert `DataStore`, optional per Feature/`cfg(any(test, feature="debug-tools"))`): Die drei
OData-Draft-Operationen, wie sie von `YamlStore` implementiert werden:
- `draft_edit(set_name, active_key)` – Der aktive Sat wird in seinen Draft umgewandelt und geschrieben (Result = Draft-Objekt).
- `draft_activate(set_name, draft_key)` – Der abgeschlossene Draft Sat wird aktiv, der Draft Sat gelöscht (Result = aktivierter Sat).
- `draft_prepare(set_name, draft_key)` – Gibt den aktuellen Draft/Active-Stand zurück (Platzhalterverhalten, kein Determinations-Logik).

Angegeben sind auch **mod-intene Integrationstests** (CRUD, Patch-Merge, Filter/OrderBy, skip/top-Pagination,
Persist-Across-Reopen, `OData $expand` für Sammlung & single-navigation). Sie validieren das komplette CRUD- und
Draft-Ablauf gegen eine temporäre YAML-Store.

### 4.2 `datastore.rs`

#### `DraftAction` (Enum)
Die drei OData-Draft-Aktionen: `Edit`, `Activate`, `Prepare`.

#### `DraftAdministrativeData` (Serde-Struktur)
Draft-spezifische Meta: `DraftUUID`, `DraftIsCreatedByMe`, `InProcessByUser`, `LastChangedByUser`. Alle Felder haben serde-
`default`, damit sie optional sind.

#### `DraftMessage`
Ein Nachrichtensatz (analog RAP-Draft-Messages): `target`, `message`, `code`, `numericSeverity`.

#### `DataRecord` (Kern-Datenmodell)
Hält Metadaten um ein freies JSON-Field-Mapping (`#[serde(flatten)] Map<String, Value>` für die Geschäftsfelder):

| Feld                    | serde-Name        | Typ                            | Default |
|-------------------------|-------------------|-------------------------------|---------|
| `ID`                    | `ID`              | `Uuid`                        | `new_v4`|
| `IsActiveEntity`        | `IsActiveEntity` | `bool`                        | `false` |
| `HasActiveEntity`       | `HasActiveEntity` | `bool`                        | `false` |
| `HasDraftEntity`        | `HasDraftEntity` | `bool`                        | `false` |
| `DraftAdministrativeData` | `DraftAdministrativeData` | `Option<…>`           | `None`  |
| `DraftMessages`         | `DraftMessages`  | `Vec<DraftMessage>`           | `[]`    |

**Methods:**
- `new(id, is_active)` – reines Skelett.
- `from_json(&Value)` – zerlegt ein JSON-Objekt in ID/Meta + Business-Felder (`uuid` wird gelöst, falls keine gültige ID).
- `get(segment)` – access auf System-Keys *oder* Business-Feld.
- `touch(s)` – für OData `$select`/`ReadProperty`: Erweitert Felerden-Platzhalter `_<name>` (außer System-Keys).
- `id()`, `is_active_entity()`, `has_active_entity()`, `has_draft_entity()`, `draft_admin_data()`, `draft_messages()`.
- `activate()` – setzt active-Sat-Zustand: active=true, has_active=false, has_draft=false, Draft-Admin/Dates geleert.
- `merge(patch)` – PATCH: Werte in den Field-Map, `entry.or_insert(Null) = value`.
- `edit()` – Draft-Transition: active=false, has_active=true, has_draft=false, neue Draft-Admin-Meta (neue DraftUUID, CreatedByMe=true).
- `create_draft()` – wie `edit()`, zusätzlich `has_active_entity=false` (für einen *neuen* Draft ohne bestehenden Aktiv-Sat).
- `core_json()` – liefert ID + Business-Felder (nur Kern, kein Draft-Meta) als `Value`.
- `Index/IndexMut<&str>` – Feld-zugiff via Index-Syntax.
- `From<&DataRecord> for EntityKey` – Key aus Record ableiten.

#### `Navigation`
Steuerung einer OData-$expand-Beziehung:
- `single: bool` – Single- (true) vs Sammlungsnavigation (false).
- `from_set / from_field / to_set / to_field` – Paare von Quell-/Zieldimensionen.
- Helfer `new(single)` / `from(set, field)` / `to(set, field)` (fl chaining).

#### `DataStore`-Trait
Der store-abstrahierende Kern. Obligatorische Operationen plus zwei abstrahierte Navigations-Methoden:

- **Collection:** `get_collection(set_name, query) -> Vec<DataRecord>`, `count(set_name, query) -> usize`.
- **Single-Entity CRUD:** `read_entity`, `create_entity`, `patch_entity`, `delete_entity`, `get_property`.
- **Sibling-Entity:** `read_sibling_entity` (aktuell `todo!()`).
- **Property:** `get_property`.
- **Persistence:** `commit()` (bei `YamlStore` ein No-Op).
- **Navigierung:** `navigation(single) -> &mut Navigation`, `set_navigations(Vec<Navigation>)`.

**Default-Methoden (kein Implementierungs-Zwang):**
- `update_entities(set, [(key, patch)])` – mappt das Update-Pair über `patch_entity`.
- `seed_records(set, records)` – erstellt aus einem Seed-Vec Records.

#### `DataStoreDebugExt`-Trait
Eine **optionale** Debug/Inspection-Erweiterung (`get_records(set) -> Vec<Value>`, ungefilterter Full-Table-Scan) wird
bewusst vom Kern-Trait getrennt, damit eine spätere Postgres-Implementation nicht gezwungen ist, einen nicht-effizient
gefilterten Full-Scan anzubieten.

### 4.3 `yaml_store.rs`

`YamlStore` implementiert `DataStore` + `DraftDataStore`. Zwischenspeicher (Cache) ist bewusst commented-out (kein Cache).

**Fields:** `root: PathBuf`, `current_change: RwLock<Option<Uuid>>`, `navigations: RwLock<Vec<Navigation>>`.

**Key-Methoden:**
- `open(root)` – Kontextfrei öffnen; erstellt `current_change`/`navigations` leert. Kein eager Load; erstellt benötigte
  Verzeichnisse beim Schreiben.
- `get_change()` – liefert oder generiert (`new_v4`) den Change-Tracker-UUID.
- `path_for_set(set, draft)` – `root/<set>/<draft|active>`.
- `path_for(set, key)` – `.../<set>/<draft|active>/<storage_id>.yaml`.
- `write_record` – atomares Schreiben über `tmp`+`rename`.
- `remove_record` – Datei (falls vorhanden) löschen.
- `load_records(set)` – lädt alle Drafts, dann nur Active-Records ohne Draft-Dopplung.

**`get_collection`-Pipeline:**
1. Records laden (`load_records`).
2. `$select` → `r.touch(s)` für jedes ausgewählte Feld.
3. `$expand` → für jede Navigation mit passendem `navigation_property`:
   - Sammlgavigation: Filter-Query `by id = row.id` auf Zieldimension.
   - Single: Filter-Query `by <from_field>_ID = row.<from_id_field>`.
   - `expand_single_row` schreibt das Ergebnis in `<from_field>_ID`.
4. `$filter` → `filter_eval::evaluate` auf jedem Record, `retain` die wahren.
5. `$orderby` → für jedes Orderby-Feld (reverse durchlaufend, damit frühere Präzedenz gehalten wird) sortiere per
   `compare_json`.
6. `$skip` → `drain`, `$top` → `truncate`.
7. `count` ignoriert `top`/`skip`/`select`/`expand`, zählt nur die Base-Colektion.

**Draft-Operations (`DraftDataStore`):**
- `draft_edit` – liest aktiven Sat (leere Query), ruft `edit()`, schreibt.
- `draft_activate` – liest Draft-Sat, `activate()`, schreibt, löscht den Draft-Sat.
- `draft_prepare` – liest aktuellen Sat und gibt ihn zurück.

**Limitation:** `patch_entity` ist Read-then-Merge – kein Optimierungs-/Transaktionsschutz (bewusst für Prototyp).
`$expand`-Sammlung liefert bei >1 Treffer `todo!()`; `read_sibling_entity` ist `todo!()`.

### 4.4 `key.rs`

**`EntityKey`** (derzeit: Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize):
- Felder: `id: Uuid`, `is_active_entity: bool`.
- `new(id, is_active)` / `new_active(id_string)` – aktiven Key per String.
- `storage_id()` – String-für Den Dateinamen.
- `sibling()` – flipped die active-Flag (aktiv ↔ draft).
- `new_from_json(&Value)` / `From<&PathSegment>` – Key aus JSON oder OData-Path ableiten.

`EntityKey` ist die Identifikationseinheit aller Single-Entity-Operationen und der Draft-Bezug (Schrift/Activierung).

### 4.5 `filter_eval.rs`

`evaluate(record, expr) -> Value` interpretiert OData-`Expr`. Zusätzlich `compare_json_for_sort` für Sort.

**Implementiert (aktiv):**
- Identifier (einzsegmig; mehrsegmig `todo!()`) und Path-Expression (mehrere Segmente per `.get`).
- Literale: String, Number, Bool, Guid, Null.
- Logisch: `UnaryNot`, Binär `And`, `Or`, `Eq`, `Ne`. Vergleichsoperatoren `Gt/Ge/Lt/Le` sind `todo!()`.
- Value-Funktionen: `tolower`, `toupper`, `trim`, `length`, `concat`.
- Bool-Funktionen: `contains`, `startswith`, `endswith`, `substringof` (V2-Älteste Variante, Argumentreihenfolge angepasst).

**Lücken:** `evaluate_operand` ist `todo!()`; `FunctionCall` wird `todo!()` zurückgegeben. Die Engine ist bewusst minimal – nur was der Prototyp benötigt.

### 4.6 `error.rs`

**`StoreError`** (enum; Debug, Display, Error, `From<io::Error>`):
- `NotFound(String)`, `Io(std::io::Error)`, `Serialize(String)`, `Deserialize(String)`, `Metadata(String)`, `Conflict(String)`.

Die Meldungen werden deutsch formatiert (Display).

## 5. Datenfluss (Beispiel: Collection-Read mit Filter/Expand)

1. Server-Code baut eine `odata::query::ODataQuery` (Filter, Orderby, Expand, Select, Skip, Top).
2. `YamlStore::get_collection` lädt Base-Records (`load_records`) für die Sammlung.
3. `$select` triggert `touch` (Erweiterungen-Feld-Platzhalter).
4. `$expand` ruft für jede passende Navigation eine rekursive Collections-Abfrage auf und fillt das Navigationsfeld
   (`expand_single_row`). Bei `get_collection` wird `expand` in `count` deaktiviert.
5. `$filter` → `filter_eval::evaluate` filtert die Base-Collection.
6. `$orderby` sortiert (mehrere Pass per reverse-Iteration).
7. `$skip`/`$top` paginiert die Resultat-Vec.
8. Returns `Vec<DataRecord>` (je Record ein YAML-Serialisiertes-Objekt inkl. System-Keys + Business-Felder).

## 6. Bekannte Vereinfachungen & Limitationen

- **Write-Through, `commit` = No-Op:** Sofortiges Schreiben, kein Transaktionsschutz/Dirty-Tracking. Für den Prototypen ausreichend.
- **Keine Optimierungs-/Konflikt-Erkennung** (`patch_entity` ist optimistisch).
- **`$expand`-Sammlung:** Mehrfache Treffer liefert `todo!()`; `has_one`-single-navigation ist der Hauptpfad.
- **`read_sibling_entity`** nicht implementiert.
- **`filter_eval`:** `evaluate_operand` + alle relationalen Vergleichsoperatoren + `FunctionCall` sind `todo!()`.
- **Kein Parser für OData-URI-Strings** – der Aufrufer füllt `ODataQuery` aus.
- **`$apply` (Aggregation/GroupBy, OData V4 Analytics):** nicht abgebildet.
- **`draft_prepare`:** Nur Platzhalterverhalten, keine Validations-/Determinations-Logik.
- **No Cache / no eager Load.**

## 7. Migration Richtung Postgres

Da der `DataStore`-TraitImplementation-abstrakt ist, müsste nur `YamlStore` ausgetauscht werden:
```rust
pub struct PostgresStore { pool: /* … */ }
impl DataStore for PostgresStore { /* gleiche Methodensignaturen, SQL statt YAML */ }
```
`EntityKey`, `ODataQuery` bleiben wiederverwendbar; `datastore.rs` wird durch einen SQL-Query-Beder (als Ersatz für
`filter_eval.rs`) erweitert. Aufrufer-Code bleibt unverändert.

---

*Erstellt aus `src/*.rs`, `README.md`, `tests/*.rs` und `examples/*.rs`. Keine Code-Änderung Bestandteil der Erstellung.*
