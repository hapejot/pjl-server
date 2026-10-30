# Customizing: Möglichkeiten und Howto

Die Oberfläche einer App wird über drei Ebenen angepasst. Die erste Ebene ist deklarativ und
der Normalfall. Die zweite Ebene sind Assets (Fragmente, JS, Controller). Die dritte Ebene ist Logik
in Rhai oder Rust.

| Ebene | Wo | Wann | Neustart nötig |
|---|---|---|---|
| Deklarativ | `ddic.json` (Blöcke `ui`, `side_effects`, `link` …) | Formate, Ausblenden, Abschnitte, Spalten, Tabs | ja |
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

## 7. App-Module und Controller-Erweiterungen

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
  `<controller>.controller.js` (siehe Abschnitt 8).

**Achtung:** Module und Controller gelten derzeit für das ganze Manifest, nicht nur für die
Seiten einer App. Eine ListReport-Erweiterung wirkt daher auf jeder ListReport-Seite der App.

## 8. Asset-Verzeichnis

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

## 9. Rhai-Skripte (Hooks)

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

## 10. Rust-Extensions

Für Routen, Actions und Logik, die nicht in Rhai passt, implementiert man den Trait
`fiori_server::extension::Extension` (`fiori-server/src/extension.rs`) und reicht die Erweiterung
an `server::run_with` weiter. Beispiel: `video-store/src/bin/video-runtime.rs`.

Der Trait bietet u. a. `routes`, `object_page_content`, `list_views`, `list_columns`, `hidden_fields`,
`ui5_modules`, `controller_extensions`, `hooks`, `actions`. Für reine Anzeige ist das nicht nötig,
das lässt sich über `ui` lösen.

## 11. Konfiguration beim Start

| Option | Env | Zweck |
|---|---|---|
| `--ddic` | `DDIC` | Datenmodell (Pflicht im Runtime-Modus) |
| `--ddic-overlay` | `DDIC_OVERLAY` | Overlay zur `ddic.json` (Listenansichten, Facetten) |
| `--ext-dir` | `EXT_DIR` | Asset-Verzeichnis je App |
| `--scripts-dir` | `SCRIPTS_DIR` | Rhai-Skripte |

Ein Overlay (`overlay/ddic.overlay.json`) ergänzt die Basis, ohne sie zu verändern. Es eignet sich für
Dinge, die nicht bei jeder Migration neu erzeugt werden sollen.

## 12. Arbeitsablauf

1. Gewünschte Änderung zuordnen: Formatierung, Abschnitt, Spalte, Tab, Skript oder Code.
2. Deklarativ: `ddic.json` anpassen, Schema prüft beim Start (unbekannte Schlüssel sind Fehler).
3. Assets: Dateien unter `<EXT_DIR>/<app>/` anlegen, Verweise in `ddic.json` prüfen.
4. Server neu starten (Ausnahme: Rhai-Skripte werden nachgeladen).
5. Browser: bei geänderten JS-Dateien den Cache leeren, sonst läuft die alte Version weiter.
6. Manifest prüfen: `GET /apps/<Entität>/manifest.json` (Modul-Liste, Abschnitte, Views).

Migration der Formate mit `jq` (ohne Server-Neustart, Ergebnis als neue Datei):

```
jq --argjson formats '{"Video":{"duration_seconds":"duration","bitrate":"bitrate"}}' \
   -f scripts/ddic-migrate-ui.jq video-store/ddic.json > video-store/ddic.new.json
```

## 13. Bekannte Einschränkungen

* **Designer: `ui`-Blöcke sind nicht editierbar.** Beim Speichern im Designer werden die
  ursprünglichen Atom-Felder übernommen, damit bleiben vorhandene `ui`-Blöcke erhalten. Neue Werte
  lassen sich im Designer aber nicht setzen; das folgt mit der Designer-Überarbeitung.
* **`video-ddic` erzeugt `ddic.json` neu.** Dabei gehen alle `ui`-Blöcke verloren. Das Werkzeug
  muss angepasst werden, bevor es erneut läuft.
* **Module und Controller wirken global** (siehe Abschnitt 7).
* **Intent-Links** für Object Pages verwenden die Aktion `Detail` mit Pflichtparameter `ID`.
  Ein Link ohne `ID` löst die App nicht auf.
* **Join-Entitäten** (M:N ohne eigene App) bekommen keinen Schlüssel-Fallback für Sprünge.
  Ein Link aus einer solchen Tabelle kann auf den Zeilenschlüssel zurückfallen. Bekannt und akzeptiert.
