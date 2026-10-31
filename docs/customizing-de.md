# Customizing: Möglichkeiten und Howto

Die Oberfläche einer App wird über drei Ebenen angepasst. Die erste Ebene ist deklarativ und
der Normalfall. Die zweite Ebene sind Assets (Fragmente, JS, Controller). Die dritte Ebene ist Logik
in Rhai oder Rust.

| Ebene | Wo | Wann | Neustart nötig |
|---|---|---|---|
| Deklarativ | `ddic.json` (Blöcke `ui`, `side_effects`, `link`, `components`, `component_uses` …) | Formate, Ausblenden, Abschnitte, Spalten, Tabs, wiederverwendbare Bausteine | ja |
| Assets | `<EXT_DIR>/<app>/` | eigene Fragmente, Module, Controller | ja (Browser-Cache beachten) |
| Logik | Rhai-Skripte (`SCRIPTS_DIR`), Rust-Extensions | Hooks, Routen, Actions | Rhai: nein (Reload). Rust: ja, Neubau |

Die Vorgaben stammen aus `fiori-server/src/ui_ext.rs`, `spec/src/entity_spec.rs` und `spec/src/app.rs`.
Die Beispiele beziehen sich auf den Video-Store (`video-store/`).

---

## 1. Anzeige-Formate für Felder

Atom-Felder bekommen einen `ui`-Block mit `format`. Erlaubt sind `duration`, `bytes`, `bitrate`,
`date`, `datetime`. Unbekannte Formate brechen das Laden ab.

```json
{ "Atom": { "name": "duration_seconds", "label": "Dauer", "edm_type": "Edm.Double",
            "ui": { "format": "duration" } } }
```

Ergebnis:

* In der Liste erscheint eine formatierte Spalte neben dem Rohfeld. Das Rohfeld wird ausgeblendet
  (bleibt für Sortierung und Filter erhalten).
* Auf der Object Page entsteht der Abschnitt "Technische Daten" mit allen formatierten Feldern.

Zahlen und Beträge werden **nicht** über Formate gesteuert. Tausendertrenner und Dezimalzeichen
übernimmt Fiori Elements über das Locale des Benutzers.

## 2. Beträge und Mengen (Measures)

Ein Measure ist ein Wert mit Begleitfeld für Währung oder Einheit. Im Spec ist es `FieldSpec::Measure`
mit `unit_field` und `kind` (`Currency` oder `Unit`). Die Ausgabe nutzt
`Org.OData.Measures.V1.ISOCurrency` bzw. `.Unit` mit Pfad auf das Begleitfeld.

Das Begleitfeld (z. B. `currency`) ist ein eigenes Atom-Feld mit dem Währungscode, pro Satz.

## 3. Ausblenden

```json
"ui": { "hidden_fields": ["secret"] }
```

Ausgeblendete Felder fehlen im Standard-Formular und in der Liste. Rohfelder mit `format` werden
automatisch ausgeblendet.

## 4. Feldoptionen, die schon im Spec stehen

| Option | Wirkung |
|---|---|
| `link: true` | Wert ist eine URL, wird als Link dargestellt |
| `multiline: true` | Langtext, mehrzeilige Eingabe |
| `side_effects: ["<feld>"]` | Nach einer Änderung wird das Zielfeld nachgeladen (`Common.SideEffects`, auf Entitätsebene) |
| `value_list` | Dropdown / Wertehilfe |

`side_effects` beispiel (aus `workbench/ddic.json`):

```json
{ "Atom": { "name": "industry", "side_effects": ["industry"] } }
```

## 5. Object-Page-Abschnitte und Facetten

Im `ui`-Block einer Entität:

```json
"ui": {
  "sections": [
    { "key": "VideoPlayer", "title": "Video", "fragment": "Player",
      "anchor": "GeneralSection", "placement": "After" }
  ],
  "hide_facets": ["_VideoThumbnailsSection"]
}
```

* `key`: Id des Abschnitts.
* `title`: Text, auch `{{i18nKey}}`.
* `fragment`: Dateiname ohne `.fragment.xml`, liegt unter `<EXT_DIR>/<app>/fragment/`.
* `anchor` / `placement`: Position relativ zu einem Abschnitt (Standard: `GeneralSection`, `After`).
* `hide_facets`: Standardfacetten der Navigation ausblenden (Id = `<navigation>Section`).

## 6. Listenspalten und Listen-Tabs

```json
"ui": {
  "list_columns": [
    { "key": "VideoPreview", "header": "{{previewColumnTitle}}", "fragment": "PreviewColumn",
      "anchor": "name", "placement": "Before", "width": "120px" }
  ],
  "list_templates": [
    { "key": "TagSearch", "label": "Tag-Suche", "fragment": "TagSearch" }
  ]
}
```

* `list_columns`: Custom Column, Fragment-Inhalt ist die Zelle.
* `list_templates`: zusätzlicher Tab (View) in der ListReport-Seite mit eigenem Fragment.
  Die Standardansicht bleibt erhalten.
* Zusätzliche Views ohne eigenes Fragment: `list_views` im Entity-Schema (`name`, `columns`).

## 7. Komponenten (wiederverwendbare Bausteine)

Eine Komponente bündelt alles, was ein wiederkehrendes Muster braucht: Felder, Regeln, Anzeige
und Bearbeitung. Beispiel: ein **Zeitraum** aus Beginn, Ende und Dauer. Die Komponente beschreibt
nur *Rollen*, keine konkreten Felder. Dieselbe Komponente lässt sich in mehreren Entitäten oder
mehrfach in einer Entität verwenden, ohne etwas zu kopieren.

Es gibt drei Teile in der `ddic.json`:

**a) Definition** (`components`, einmal, global):

```json
"components": [
  { "name": "TimeRange", "kind": "time_range", "script": "components/TimeRange",
    "unit": "minutes", "step_minutes": 5 }
]
```

| Schlüssel | Bedeutung |
|---|---|
| `name` | Eindeutiger Name der Komponente |
| `kind` | Art. `time_range` hat die Rollen `start` (DateTimeOffset), `end` (DateTimeOffset), `duration` (Int32) |
| `script` | Rhai-Modul mit den Regeln, relativ zu `SCRIPTS_DIR` (ohne `.rhai`) |
| `unit` | Einheit der Dauer. Derzeit nur `minutes` |
| `step_minutes` | Raster der Zeitpunkte und der Dauer in Minuten. Muss 1 bis 60 sein und 60 teilen |

**b) Verwendung** (`component_uses`, je Einsatz):

```json
"component_uses": [
  { "entity": "Appointment", "component": "TimeRange", "instance": "termin",
    "fields": { "start": "starts_at", "end": "ends_at", "duration": "duration_minutes" },
    "labels": { "start": "Beginn", "end": "Ende", "duration": "Dauer" },
    "section_title": "Zeitraum" }
]
```

| Schlüssel | Bedeutung |
|---|---|
| `entity` | Entität, in der die Komponente liegt |
| `instance` | Schlüssel des Einsatzes in dieser Entität. Muss eindeutig sein |
| `fields` | Rolle → Feldname. Alle Rollen der Komponente müssen belegt sein |
| `labels` | Rolle → Beschriftung (optional, sonst Standardtext) |
| `step_minutes` | Überschreibt das Raster der Komponente für diesen Einsatz (optional) |
| `section_title` | Titel des Abschnitts auf der Object Page. Ohne Titel entsteht kein Abschnitt |

Eine zweite Verwendung mit anderen Feldern (z. B. Ist-Zeiten) braucht nur einen weiteren Eintrag
mit eigener `instance`. Felder, Abschnitt und Regeln werden dann je Einsatz erzeugt.

**c) Regeln** (Rhai-Modul unter `SCRIPTS_DIR/components/`):

```rhai
fn before_create(ctx, cfg) { apply_rules(ctx, cfg) }
fn before_patch(ctx, cfg)  { apply_rules(ctx, cfg) }
```

`cfg` enthält die Zuordnung aus der DDIC: `cfg.instance`, `cfg.fields.start`, `cfg.fields.end`,
`cfg.fields.duration`, `cfg.step_minutes`. Die Regeln im Modul `components/TimeRange.rhai` sind
Ende ↔ Dauer, Rundung auf das Raster und Fehler bei negativen Dauern. Das Modul kennt keine
Feldnamen, nur die Rollen.

### Was beim Laden passiert

1. **Expansion** (`spec/src/component.rs`, `DataDictionary::expand_components`): aus jeder
   Verwendung entstehen Atom-Felder mit Typ und Beschriftung, die `side_effects` (jede Rolle
   lädt die beiden anderen nach) und die `hidden_fields` des Formulars. Ein Feld, das schon
   existiert, muss den erwarteten Typ haben, sonst bricht der Start ab. Die Expansion ist
   idempotent, eine erneut gespeicherte `ddic.json` bleibt stabil.
2. **Bindung** (`fiori-server/src/script.rs`, `bind_components`): jede Verwendung wird an das
   Entitäts-Set gebunden, mit Skript und Konfiguration. Vor jedem Schreibvorgang laufen die
   gebundenen Regeln vor dem Entitätsskript (`<Entität>Set.rhai`).
3. **UI**: siehe Abschnitt 8.

Fehler (unbekannte Rolle, Feld mit falschem Typ, Raster ohne Teiler von 60, fehlendes Skript)
brechen den Start mit einer Meldung ab. Das ist gewollt.

### Verwendung in einer neuen Entität

1. In `entities` die Felder der Entität wie gewohnt pflegen, ohne die Komponentenfelder.
2. Eintrag in `component_uses` mit der neuen `entity`, `instance` und `fields`.
3. Start, dann im Manifest prüfen (`GET /apps/<Hauptentität>/manifest.json`).

**Achtung:** Abschnitte entstehen nur für die Hauptentität einer App. Eine Komponente in einer
Nebenentität erzeugt keinen Abschnitt auf der Object Page.

### Einschränkungen

* Nur die Einheit Minuten.
* Die Komponenten-Regeln laden bei Dateiänderungen neu, die Bindungen (`component_uses`) erst beim Start.
* Hat die App schon einen eigenen Object-Page-Controller (Abschnitt 9), wird der Komponenten-Controller
  nicht eingetragen. Der Manifest-Eintrag ist je Ziel nur einmal möglich.

## 8. Virtuelle Fragmente: wie sie entstehen

Fragmente, die der Server erzeugt, liegen nirgends auf der Platte. Sie entstehen beim Start aus
der DDIC und werden bei Bedarf als XML ausgeliefert.

Ablauf:

1. **Start**: `UiExtension::new` (`fiori-server/src/ui_ext.rs`) liest die DDIC und baut zwei
   Dinge im Speicher:
   * für Felder mit `ui.format` das Fragment für die Spalte bzw. den Abschnitt "Technische Daten",
   * für jede Komponenten-Verwendung mit `section_title` das Fragment-XML (`component_fragment`)
     und den Abschnitt.
2. **Manifest**: Die Object Page verweist im Abschnitt auf `fiori.ext.fragment.<Name>`, die
   Listenspalte auf `fiori.ext.fragment.<Feld>__<Format>`. Das Manifest kommt aus
   `GET /apps/<Entität>/manifest.json`.
3. **Anfrage**: FE lädt `…/ext/fiori/fragment/<Name>.fragment.xml`. Die Route in `ui_ext.rs`
   sucht den Namen im Speicher und liefert das XML.

Die Namen folgen festen Mustern:

| Muster | Erzeugt | Beispiel |
|---|---|---|
| `<Feld>__<Format>` | Textfeld mit Formatter für eine Listenspalte | `duration_seconds__duration` |
| `Formatted__<Entität>` | Abschnitt "Technische Daten" mit allen formatierten Feldern | `Formatted__Video` |
| `Component__<Entität>__<Instanz>` | Formular der Komponente (Anzeige/Bearbeiten, Konfiguration im `customData`) | `Component__Appointment__termin` |

Die Konfiguration einer Komponente steckt im Fragment als `customData` `timeRange`:

```
instance=termin;start=starts_at;end=ends_at;duration=duration_minutes;step=5
```

Der Controller `fiori.ext.ComponentsController` (Datei `fiori-server/src/ui_components_controller.js`)
liest sie aus, bindet die Eingabefelder an seine Logik und lädt nach jeder Änderung die Felder nach.
Er ist für alle Komponenten gleich, nur die Konfiguration unterscheidet sich.

Prüfen lässt sich das direkt:

```
curl http://<server>/ext/fiori/fragment/Component__Appointment__termin.fragment.xml
curl http://<server>/apps/Appointment/manifest.json
```

Folgen daraus:

* Änderungen an `ui`, `components` oder `component_uses` wirken erst nach einem Neustart.
* Für ein neues Fragmentmuster braucht es eine Änderung in `ui_ext.rs`. Das lässt sich nicht
  allein über die `ddic.json` erreichen.
* Fragmente aus Dateien (`<EXT_DIR>/<app>/fragment/`, Abschnitt 10) und virtuelle Fragmente
  können nebeneinander bestehen. Sie unterscheiden sich nur im Namensraum.

## 9. App-Module und Controller-Erweiterungen

Auf App-Ebene (`apps[]` in `ddic.json`):

```json
"ui": {
  "modules": ["Player", "TagSearch"],
  "controller_extensions": [
    { "target": "sap.fe.templates.ObjectPage.ObjectPageController", "controller": "ObjectPageController" },
    { "target": "sap.fe.templates.ListReport.ListReportController", "controller": "TagSearchController" }
  ]
}
```

* `modules`: Datei `<app>/ext/<name>.js` als UI5-Modul `<app>.ext.<name>`. Im Video-Store also
  `videos/ext/TagSearch.js` → Modul `videos/ext/TagSearch`.
* `controller_extensions`: `controller` ist der Dateiname ohne `.controller.js`. Die Datei heißt
  `<controller>.controller.js` (siehe Abschnitt 10).

**Achtung:** Module und Controller gelten derzeit für das ganze Manifest, nicht nur für die
Seiten einer App. Eine ListReport-Erweiterung wirkt daher auf jeder ListReport-Seite der App.

## 10. Asset-Verzeichnis

Der Server startet mit `--ext-dir` bzw. `EXT_DIR`. Jede App bekommt ein Unterverzeichnis
mit dem App-Namen (`apps[].name`):

```
<EXT_DIR>/
  videos/                        ← App "videos"
    Player.js                    → Modul videos/ext/Player
    TagSearch.js                 → Modul videos/ext/TagSearch
    ObjectPageController.controller.js
    TagSearchController.controller.js
    fragment/
      Player.fragment.xml        → "fragment": "Player"
      Thumbnails.fragment.xml
      PreviewColumn.fragment.xml
      TagSearch.fragment.xml
```

Die Dateien werden unter `/ext/<app>/` ausgeliefert. Der UI5-Namespace ist `<app>.ext`.

Fragmente sind normale XML-Dateien mit `core:FragmentDefinition`. Verweise auf Funktionen erfolgen
über globale Namen, die das Modul in `ObjectPath` setzt, z. B. `videos.ext.Player.seek`.

Die Formatter der eingebauten Formate liegen im Namespace `fiori.ext.Format`. Eigene Formatter
gehören in das eigene App-Verzeichnis und werden über einen eigenen Namespace angesprochen.

### Im Container (Workbench)

In `workbench/compose.yaml` zeigt `EXT_DIR` auf `/app/model/ext`, das Verzeichnis `workbench/ext/`
ist als Volume eingebunden (nur lesend). Eigene Fragmente, Module und Controller legt man also unter
`workbench/ext/<app>/` ab, z. B. `workbench/ext/appointments/fragment/Notes.fragment.xml`. Das Verzeichnis
`workbench/ext/` enthält eine `.gitkeep`, damit es nach einem Checkout vorhanden ist. Fehlt es, legt
Docker es root-eigen an, und die Dateien lassen sich nicht ohne Weiteres pflegen.

Nach dem Anlegen den Container neu starten und `GET /apps/<Entität>/manifest.json` prüfen.

## 11. Rhai-Skripte (Hooks)

Skripte liegen in `SCRIPTS_DIR` als `<Entitaet>.rhai`. Sie werden beim Start geladen und bei
Dateiänderung neu eingelesen (kein Neustart nötig).

```rhai
fn before_patch(ctx) {
    let industry = ctx.payload.industry;
    if industry != () {
        ctx.payload.industry = industry.to_upper();
    }
    ctx
}
```

Verfügbare Funktionen: `before_<op>`, `after_<op>` (Schreiben), `before_read`, `after_read`.
`op` ist `create`, `patch`, `delete`, `read` u. a. Die Funktion muss `ctx` zurückgeben.

Die Komponenten-Regeln liegen in `SCRIPTS_DIR/components/<Name>.rhai`. Sie werden nicht über den
Dateinamen zugeordnet, sondern über `component_uses` (Abschnitt 7). Verfügbare Datumsfunktionen:
`ts_round_minutes(iso, step)`, `ts_add_minutes(iso, n)`, `ts_diff_minutes(a, b)`, `ts_valid(iso)`.
Zeitpunkte sind ISO-8601-Texte mit Offset.

Rhai-Hinweis: Eine verschachtelte `if`-Kette in einer Funktion führt zu "Expression exceeds maximum
complexity". Regeln in kleine Funktionen aufteilen.

## 12. Rust-Extensions

Für Routen, Actions und Logik, die nicht in Rhai passt, implementiert man den Trait
`fiori_server::extension::Extension` (`fiori-server/src/extension.rs`) und reicht die Erweiterung
an `server::run_with` weiter. Beispiel: `video-store/src/bin/video-runtime.rs`.

Der Trait bietet u. a. `routes`, `object_page_content`, `list_views`, `list_columns`, `hidden_fields`,
`ui5_modules`, `controller_extensions`, `hooks`, `actions`. Für reine Anzeige ist das nicht nötig,
das lässt sich über `ui` lösen.

## 13. Konfiguration beim Start

| Option | Env | Zweck |
|---|---|---|
| `--ddic` | `DDIC` | Datenmodell (Pflicht im Runtime-Modus) |
| `--ddic-overlay` | `DDIC_OVERLAY` | Overlay zur `ddic.json` (Listenansichten, Facetten) |
| `--ext-dir` | `EXT_DIR` | Asset-Verzeichnis je App |
| `--scripts-dir` | `SCRIPTS_DIR` | Rhai-Skripte |

Ein Overlay (`overlay/ddic.overlay.json`) ergänzt die Basis, ohne sie zu verändern. Es eignet sich für
Dinge, die nicht bei jeder Migration neu erzeugt werden sollen.

## 14. Arbeitsablauf

1. Gewünschte Änderung zuordnen: Formatierung, Abschnitt, Spalte, Tab, Skript oder Code.
2. Deklarativ: `ddic.json` anpassen, Schema prüft beim Start (unbekannte Schlüssel sind Fehler).
3. Assets: Dateien unter `<EXT_DIR>/<app>/` anlegen, Verweise in `ddic.json` prüfen.
4. Server neu starten (Ausnahme: Rhai-Skripte werden nachgeladen).
5. Browser: bei geänderten JS-Dateien den Cache leeren, sonst läuft die alte Version weiter.
6. Manifest prüfen: `GET /apps/<Entität>/manifest.json` (Modul-Liste, Abschnitte, Views).

Änderungen an bestehenden Dateien erfolgen von Hand. Für Massenänderungen an den Formaten gibt
es ein `jq`-Skript (Ergebnis als neue Datei, danach prüfen und übernehmen):

```
jq --argjson formats '{"Video":{"duration_seconds":"duration","bitrate":"bitrate"}}' \
   -f scripts/ddic-migrate-ui.jq video-store/ddic.json > video-store/ddic.new.json
```

## 15. Bekannte Einschränkungen

* **Designer: `ui`-Blöcke sind nicht editierbar.** Beim Speichern im Designer werden die
  ursprünglichen Atom-Felder übernommen, damit bleiben vorhandene `ui`-Blöcke erhalten. Neue Werte
  lassen sich im Designer aber nicht setzen; das folgt mit der Designer-Überarbeitung.
* **`video-ddic` ist nur Notfall-Werkzeug.** Es erzeugt `ddic.json` neu und verwirft dabei alle
  `ui`-Blöcke. Es ist nicht im Container-Image und nicht Teil des Migrationsablaufs. `ddic.json`
  wird von Hand gepflegt.
* **Module und Controller wirken global** (siehe Abschnitt 9).
* **Komponenten** wirken nur auf der Hauptentität einer App und nur mit Minuten (Abschnitt 7).
* **Intent-Links** für Object Pages verwenden die Aktion `Detail` mit Pflichtparameter `ID`.
  Ein Link ohne `ID` löst die App nicht auf.
* **Join-Entitäten** (M:N ohne eigene App) bekommen keinen Schlüssel-Fallback für Sprünge.
  Ein Link aus einer solchen Tabelle kann auf den Zeilenschlüssel zurückfallen. Bekannt und akzeptiert.
