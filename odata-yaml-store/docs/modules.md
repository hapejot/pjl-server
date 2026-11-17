# Technische Moduldokumentation: `odata-yaml-store`

`odata-yaml-store` implementiert ein `DataStore`-Backend, das jeden Datensatz
als YAML-Datei speichert. Das gemeinsame Datenmodell und die Store-Interfaces
liegen im Workspace-Crate `data-store`; der Query-AST stammt aus `odata`.

## 1. Crate-Struktur

| Datei | Aufgabe |
|---|---|
| `src/lib.rs` | Re-Exports aus `data-store` und `YamlStore` |
| `src/yaml_store.rs` | Dateiablage, Queries, Navigation und Draft-Operationen |
| `src/tests.rs` | Unit- und Integrationstests im Crate |
| `tests/basic.rs` | CRUD- und Basisverhalten |
| `tests/conformance.rs` | Conformance-Tests für `DataStore` |
| `examples/demo.rs` | CRUD-, Query-, Expand- und Draft-Beispiel |

`DataRecord`, `DataStore`, `DraftDataStore`, `EntityKey`, `Navigation` und
`StoreError` sind im Crate `data-store` definiert und werden von
`odata-yaml-store` re-exportiert. `ODataQuery` und dessen AST-Typen kommen aus
`odata::query`. Gemeinsame Query-Auswertung, Filterfunktionen und
Navigationsauflösung liegen ebenfalls in `data-store`, nicht in diesem Crate.

## 2. Ablage und Lesen

`YamlStore::open(root)` richtet den Store ein. Datensätze liegen unter
`<root>/<EntitySet>/active/` oder `draft/`; der Dateiname wird aus dem
`EntityKey` erzeugt. Ein Datensatz wird als flaches YAML-Mapping mit
Systemfeldern und fachlichen Feldern serialisiert.

Schreibvorgänge verwenden eine temporäre Datei und benennen diese anschließend
um. Der Store schreibt direkt auf die Platte; `commit()` ist ein No-op.
`get_collection` liest Drafts vor aktiven Datensätzen und blendet einen aktiven
Datensatz aus, wenn für dieselbe ID bereits ein Draft existiert. Filter,
Sortierung und Paging werden anschließend durch die gemeinsame
`data_store::query`-Implementierung angewendet.

## 3. Navigation und `$expand`

Der aufrufende Server registriert `Navigation`-Metadaten mit Quell- und
Ziel-Set, den jeweiligen Feldern, Kardinalität und `owning`. Der Store löst
diese Angaben über die Navigationshilfen in `data-store` auf.

- `$expand` einer Collection liefert alle passenden Zieldatensätze.
- Eine einwertige Navigation liefert `null`, wenn kein Ziel gefunden wird,
  den Zieldatensatz bei genau einem Treffer und einen `Conflict`-Fehler bei
  mehreren Treffern.
- `owning` kennzeichnet eine Komposition. Nur deren Collection-Kinder werden
  als Teil des Parent-Lebenszyklus behandelt.

## 4. Draft- und Kompositionsverhalten

`YamlStore` implementiert `DraftDataStore`:

- `draft_edit` akzeptiert einen aktiven Key. Ist der Datensatz ein
  Kompositionskind, muss sein Parent bereits einen Draft haben. Danach wird
  der Datensatz in den Draft-Zustand überführt und die Kompositionskinder
  werden rekursiv bearbeitet.
- `draft_activate` akzeptiert einen Draft-Key, aktiviert zuerst dessen
  Draft-Kinder rekursiv, aktiviert danach den Parent und entfernt dessen
  Draft-Datei.
- `read_sibling_entity` liest den aktiven bzw. Draft-Datensatz mit dem
  umgeschalteten `EntityKey`.
- Löschen durchläuft Kompositionsbäume. Beim Verwerfen eines Drafts wird auch
  der zugehörige aktive Baum entfernt.

Die Baumoperationen sind nicht transaktional. Schlägt eine Operation nach
bereits erfolgten Schreibvorgängen fehl, setzt der Store diese Änderungen
nicht automatisch zurück.

## 5. Bekannte Grenzen

- `draft_prepare` gibt den aktuellen Datensatz zurück; Validierungen und
  Determinations sind nicht implementiert.
- Mehrere Datensatzänderungen sind nicht als gemeinsame Transaktion
  abgesichert.
- `$apply`-Transformationen sind nicht Teil des unterstützten
  `ODataQuery`-Umfangs.
- Der Store parst keine OData-URI-Strings. Der aufrufende Server muss daraus
  ein `ODataQuery` erzeugen.

## 6. Tests und Beispiel

Im Workspace-Root:

```bash
cargo test -p odata_yaml_store
cargo run -p odata_yaml_store --example demo
```

Für den separaten Conformance-Test:

```bash
cargo test -p odata_yaml_store --test conformance
```

Das Beispiel setzt Navigationsmetadaten explizit und zeigt unter anderem
Collection- und einwertige Navigationen sowie Draft-Aktivierung.
