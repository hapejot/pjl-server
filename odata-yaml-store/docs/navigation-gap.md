# Navigation und Draft-Propagation

> Status gegen den aktuellen Quellcode geprüft am 2026-11-17. Die frühere
> Gap-Analyse beschrieb einen älteren Stand: `read_sibling_entity` und die
> Draft-Propagation für Kompositionsbäume sind inzwischen implementiert.

## Aktueller Stand

`data-store::Navigation` enthält Quell- und Ziel-Set, die jeweiligen Felder,
Kardinalität (`single`) und `owning`. Der aufrufende Server registriert diese
Laufzeit-Metadaten mit `DataStore::set_navigations`; der YAML-Store selbst
speichert sie nicht in den Datensatzdateien.

`data_store::nav::nav_links` erzeugt Links in beiden Richtungen. `owning`
kennzeichnet Kompositionen; Assoziationen bleiben davon getrennt. Der
`YamlStore` verwendet die aufgelösten Links wie folgt:

- `$expand` einer Collection liefert die passenden Zielzeilen.
- Eine einwertige Navigation liefert `null` bei keinem Treffer, den Datensatz
  bei genau einem Treffer und einen `Conflict`-Fehler bei mehreren Treffern.
- `draft_edit` prüft bei einem Kompositionskind, dass der Parent bereits im
  Draft ist. Bei einem Parent bearbeitet es danach die Kompositionskinder
  rekursiv.
- `draft_activate` aktiviert zuerst die Draft-Kinder einer Komposition und
  anschließend den Parent.
- `read_sibling_entity` lädt den Datensatz mit umgeschaltetem
  `IsActiveEntity`-Key.
- Lösch- und Verwerfungsoperationen durchlaufen ebenfalls die
  Kompositionskinder.

Die zentralen Implementierungen stehen in `odata-yaml-store/src/yaml_store.rs`
(`children`, `draft_edit_tree`, `draft_edit`, `draft_activate`) sowie in
`data-store/src/nav.rs` (`NavLink`, `nav_links`). Tests für Draft-Bäume und
Navigationen liegen in `odata-yaml-store/src/tests.rs`.

## Verbleibende Grenzen

- Die Navigationen sind Laufzeitkonfiguration des aufrufenden Servers und
  keine persistenten Beziehungen im YAML-Store.
- Die Draft- und Löschoperationen über mehrere Datensätze sind nicht
  transaktional. Ein Fehler kann auftreten, nachdem bereits ein Teil des
  Baums geschrieben oder gelöscht wurde.
- Eine einwertige Navigation mit mehreren Treffern ist ein Daten-/Metadaten-
  Konflikt und wird mit `StoreError::Conflict` gemeldet.

Die frühere Annahme, Navigationen hätten keine Kompositionskennzeichnung und
Draft-Edit/Activate würden keine Kinddatensätze propagieren, gilt für den
aktuellen Stand nicht mehr.
