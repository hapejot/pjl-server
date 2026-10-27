# Konfiguration des Fiori-Servers

Der Server hat drei Konfigurationsebenen: **Startparameter** (Port, Pfade, Auth), das **Data Dictionary** (das fachliche Modell) und die **Laufzeitdaten** (Store). Das Dictionary liegt in `ddic.json`, wird im Designer (`--designer`) gepflegt und vom Runtime-Server gelesen.

## 1. Startparameter (CLI-Flags und Env-Variablen)

Definiert in `fiori-server/src/config.rs`. Jeder Wert ist sowohl als `--flag` als auch als Umgebungsvariable verfügbar. Priorität: **Flag > Env > Default**.

Es gibt ein Binary, `pjl-runtime`. Mit `--designer` (bzw. `DESIGNER=1`) läuft es als Designer:

| Modus | Zweck | Daten |
|---|---|---|
| Laufzeit (Standard) | führt ein Modell aus | `ddic.json`, Store in SQLite/PostgreSQL |
| Designer (`--designer`) | pflegt das Modell über die Metamodell-Oberfläche | schreibt jede Änderung direkt in `ddic.json` (kein Skript, keine Datenbank) |

Die vollständige Liste der Flags steht in Anhang A.

## 2. Data Dictionary (das fachliche Modell)

Das Modell besteht aus Packages, Entities, Fields, Relations und Apps. Es wird im Designer in einem Store gehalten und ist im Quellcode in `spec/src/` typisiert:

- `data_dictionary.rs`: `DataDictionary` mit `packages`, `entities`, `relationships`, `apps`
- `entity_spec.rs`: `EntitySpec`, `FieldSpec` (Atom, Measure, Lookup), `ListView`, `AtomValueList`
- `relationship.rs`: `Relationship` (Komposition oder Assoziation, Kardinalität, Fremdschlüssel)
- `app.rs`: `App` (FLP-Kachel, Titel, Icon, Hauptentität)

Die Modelldaten werden in `ddic.json` gehalten. Der Designer (Abschnitt 2a) pflegt sie, die Runtime liest sie beim Start. Der Loader für die Store-Sets steht in `fiori-server/src/ddic_loader.rs`, die Felder in Anhang B.

## 2a. Der Designer

Der Designer ist kein eigenes Programm, sondern der Laufzeitserver mit `--designer` (Env `DESIGNER=1`). Er arbeitet ausschließlich auf `ddic.json`: keine Skripte, keine Datenbank, kein Entwurfszustand.

**Ablauf**

1. Der Designer öffnet `ddic.json` (`--ddic`, Standard `<export-dir>/ddic.json`). Fehlt die Datei, wird eine leere angelegt.
2. Die Oberfläche zeigt das eincompilierte Metamodell (`DataDictionary::new()`) mit den Entitäten `Entity`, `Field`, `Relation`, `Value`, `ValueList`, `Package`, `App`.
3. Jede Anlage, Änderung oder Löschung wird im `DdicStore` (`fiori-server/src/ddic_store.rs`) auf die Sets abgebildet, ins Dictionary übernommen, validiert und dann atomar geschrieben (Temp-Datei `ddic.json.tmp`, danach Rename). Scheitert die Validierung, bleibt die Datei unverändert und der Request bekommt einen Konflikt.
4. Lesen greift nur auf den Speicherstand zu und schreibt nichts.

**Persistierte Sets**

| Set | Bedeutung | Abbildung in `ddic.json` |
|---|---|---|
| `PackageSet` | Pakete | `packages` |
| `EntitySet` | Entitäten | `entities` |
| `FieldSet` | Atom-Felder einer Entität | `entities[].fields[].Atom` |
| `RelationSet` | Beziehungen | `relationships` |
| `AppSet` | Launchpad-Kacheln | `apps` |
| `ValueListSet` | Festwertlisten | `value_lists` |
| `ValueSet` | Werte einer Festwertliste | `value_lists[].values` |

Andere Sets sind nur flüchtig im Speicher.

**Regeln**

- Ein Objekt braucht ein gültiges Paket (`package_ID`); unbekannte Pakete werden abgelehnt.
- Eine App braucht eine vorhandene Hauptentität (`main_entity_ID`) und ein Paket.
- Löschen einer Entität entfernt ihre Felder und Beziehungen. Löschen einer Festwertliste entfernt ihre Werte und die Zuordnung an Feldern.
- Doppelte Namen (Entität, Paket, App, Feld je Entität, Code je Festwertliste) werden abgelehnt.
- Entwürfe (Draft) gibt es im Designer nicht. Draft-Aktionen liefern den aktiven Satz zurück.
- Unbekannte Schlüssel in `ddic.json` bleiben beim Schreiben erhalten. Die Datei wird neu formatiert, Schlüssel sortiert.

**Wirksamkeit**

Die Runtime liest `ddic.json` nur beim Start. Nach Änderungen im Designer den Runtime-Server neu starten.

## 2b. Festwertlisten

Festwertlisten sind für Statuswerte und ähnliche kleine, selten geänderte Auswahllisten gedacht. Sie werden nur im Designer gepflegt.

**In `ddic.json`**

```json
"value_lists": [
  { "id": "1f057ed4-cc46-48f4-87ef-f005e060063b", "name": "OpportunityStage",
    "values": [ { "code": "open", "label": "Offen", "sort": 1 } ] }
],
"entities": [ { "name": "Opportunity", "fields": [
  { "Atom": { "name": "status", "edm_type": "Edm.String", "max_length": 20,
              "value_list": { "FieldValueList": { "list_id": "1f057ed4-…", "prefer_dialog": false } } } }
] } ]
```

- `id` ist eine UUID und wird von `list_id` referenziert. Ungültige UUIDs, doppelte IDs oder Namen und doppelte Codes brechen den Start der Runtime ab.
- `prefer_dialog: false` ergibt ein Dropdown, `true` einen Suchdialog.

**Runtime**

Beim Start schreibt die Runtime alle Werte in das generierte Set `FieldValueListItemsSet` (Felder `name`, `ListID`, `value`, `label`, `sort`). Das Set wird bei jedem Start vollständig neu aufgebaut. Die Wertehilfe-Annotation des Atoms zeigt darauf und filtert auf `ListID`.

**Im Designer**

Festwertliste anlegen (`ValueListSet`), Werte anlegen (`ValueSet` mit `list_ID`), dann im Atom (`FieldSet`) über `value_list_ID` zuordnen.

## 2c. Skripte (Hooks)

Mit `--scripts-dir` (Env `SCRIPTS_DIR`) lädt die Runtime Rhai-Skripte, eine Datei `<set>.rhai` pro Entity Set. Sie können Lese- und Schreibzugriffe prüfen oder verändern:

- `before_<op>` / `after_<op>` für `create`, `patch`, `delete`, `draft_edit`, `draft_activate`, `draft_prepare`
- `before_read` (Query, z. B. `$filter` ergänzen oder reduzieren) und `after_read` (einmal pro Request; Listen dürfen Sätze ändern oder entfernen)
- `throw "Meldung"` bricht den Request mit Fehler ab. Der Count wird um entfernte Sätze verringert.

Änderungen an den Dateien werden automatisch neu geladen. Ein Skript mit Fehler bleibt wirkungslos und die vorherige Version aktiv. Der Designer nutzt keine Skripte.


## 3. Wo die Konfiguration abgelegt wird

```
pjl-runtime --designer       Metamodell-Oberfläche, schreibt sofort
        |
        v
export/ddic.json             semantisches Dictionary (einzige Wahrheit)
        |  Start des Runtime-Servers (--ddic)
        v
rel-data/data.sqlite         Fachdaten der Laufzeit
```

Wichtige Details:

- **Runtime** lädt `ddic.json` nur beim Start. Nach Änderungen im Designer muss der Runtime-Server neu gestartet werden. Ohne Dictionary bricht er mit einem Hinweis ab.
- **Schema**: Ohne `--schema` wird das Schema aus dem Dictionary abgeleitet, inklusive Migration neuer Tabellen. Mit `--schema` wird eine feste Schemadatei verwendet.
- **Store-Backend**: Standard ist SQLite unter `DATA_DIR`. Mit `STORE_PG_URL` läuft der Store in PostgreSQL.

### Persistenz-Hinweis

Der SQLite-Store läuft im Speicher und wird nur beim sauberen Beenden (SIGINT/SIGTERM) auf die Platte geschrieben (`server.rs`). Stürzt der Prozess ab, gehen Änderungen seit dem letzten Stopp verloren. Bei PostgreSQL entfällt dieses Problem.

## 4. Docker / Compose

- `Dockerfile`: Setzt `PORT=8080`, `DATA_DIR=/app/data`, `EXPORT_DIR=/app/export`, `WEB_ROOT=/app/web`. Läuft als Nutzer `appuser`.
- `compose.yaml`:
  - Designer: derselbe Laufzeit-Code mit `DESIGNER=1`, Port `3033:8080`, Volume `export` (schreibbar).
  - Runtime: Port `3034:8080`, Env `DDIC=/app/export/ddic.json`, Volume `export` **read-only**.
  - Optionaler Postgres-Dienst im Profil `dev` (Port 5433).

Der Austausch läuft über das gemeinsame Volume `export`. Der Runtime-Container startet erst nach dem Designer.

## 5. Auth

`fiori-server/src/auth.rs`: Der Server prüft keine Anmeldung. Ein vorgeschalteter Proxy (z. B. oauth2-proxy) setzt den Header `AUTH_USER_HEADER` (Standard `X-Forwarded-User`). Der Proxy muss diesen Header von Clients entfernen, sonst kann jeder Client die Identität selbst setzen. Mit `AUTH_REQUIRED` antwortet der Server ohne Header mit 401.

## 6. Erweiterungen (Code, keine Konfiguration)

`fiori-server/src/extension.rs`: Ein Binary kann `Extension`s einreichen, um zusätzliche Routen, UI5-Pfade, Object-Page-Sections und Listenspalten zu ergänzen. Das ist Code, keine Konfigurationsdatei.

## Auffälligkeiten

1. **Designer-Datei wird beim Schreiben neu formatiert**: Schlüssel werden alphabetisch sortiert, Formatierung ist Standard-JSON. Der Inhalt bleibt semantisch gleich.
2. **Listenansichten (`ListView`) werden nicht aus dem Store geladen.** `ddic_loader.rs` kennt kein `ListViewSet`. Listenansichten kommen aktuell nur aus Rust-Code bzw. einer `ddic.json`, nicht aus der Designer-Oberfläche.
3. **Store nur bei sauberem Stopp persistiert** (siehe Abschnitt 3).
4. **Runtime lädt Modelländerungen nicht live nach.** Neustart nötig.
5. **`fiori-server/entities.yaml`** wird von keiner Datei im Projekt referenziert (geprüft per Suche in `*.rs`, `*.toml`, `Dockerfile*`, `*.yaml`). Vermutlich eine Altlast.

---

## Anhang A: Startparameter

| Flag | Env | Default Designer | Default Runtime | Beschreibung |
|---|---|---|---|---|
| `--port` | `PORT` | 3033 | 3034 | HTTP-Port |
| `--bind` | `BIND` | 0.0.0.0 | 0.0.0.0 | Lauschadresse |
| `--designer` | `DESIGNER` | aus | aus | Designer-Modus: schreibt `ddic.json` |
| `--data-dir` | `DATA_DIR` | `rel-data` | `rel-data` | Verzeichnis für `data.sqlite` |
| `--web-root` | `WEB_ROOT` | `web` | `web` | Statische UI5-Assets |
| `--ddic` | `DDIC` | keiner | `<export-dir>/ddic.json` | Dictionary-Datei (Runtime Pflicht) |
| `--schema` | `SCHEMA` | abgeleitet | abgeleitet | Feste Schemadatei (JSON) |
| `--export-dir` | `EXPORT_DIR` | `export` | `export` | Verzeichnis, aus dem `ddic.json` gelesen wird |
| `--scripts-dir` | `SCRIPTS_DIR` | – | – | Rhai-Skripte pro Set (Hooks) |
| `--auth-user-header` | `AUTH_USER_HEADER` | `X-Forwarded-User` | `X-Forwarded-User` | Header des Auth-Proxys |
| `--auth-required` | `AUTH_REQUIRED` | aus | aus | Ohne Header: 401 |
| `--store-pg-url` | `STORE_PG_URL` | – (SQLite) | – (SQLite) | PostgreSQL-Verbindung |


## Anhang B: Modell-Sets im Designer-Store

Die Felder, die der Loader liest (`ddic_loader.rs`):

**PackageSet**: `name`, `title`, `description`

**EntitySet**: `name`, `plural` (Standard: `<name>s`), `package_ID`

**FieldSet**: `entity_ID`, `name`, `label`, `type` (Edm-Typ, Standard `Edm.String`), `role`, `max_length`, `precision`, `scale`, `computed`, `immutable`
- Felder mit Suffix `_ID` oder mit Rolle `lookup`, `association`, `composition`, `navigation` werden als Relation behandelt und nicht als Atom übernommen.

**RelationSet**: `name`, `one_ID`, `many_ID`, `source_field`, `target_field`, `source_cardinality`, `target_cardinality`, `owned`
- Kardinalität mit `N`, `n`, `*`, `many`, `0..*`, `1..*`, `0..n`, `1..n` gilt als Mehrfach-Seite.

**AppSet**: `name`, `title`, `subtitle`, `icon`, `main_entity_ID`, `package_ID`
- Die Apps erscheinen als Kacheln im Launchpad (`flp.rs`, Route `/flp/`). Icons im Format `sap-icon://...`.

**ValueListSet**: `name` (Festwertliste, ID ist die Listen-UUID)

**ValueSet**: `value` (Code), `label`, `sort`, `list_ID`

**FieldSet** zusätzlich: `value_list_ID` (Zuordnung zu einer Festwertliste)

Der frühere Hook `EntityDefaultsHook`, der beim Anlegen einer Entität `name` und `title` erzeugte, ist entfernt. Felder werden nur noch explizit angelegt.

## Anhang C: Dictionary-Eigenschaften

Nicht alles davon ist im Designer-Store abbildbar (siehe Auffälligkeit 2).

**Entity (`EntitySpec`)**
- `name`, `package`, `type_name_plural`
- `fields`: Liste von `FieldSpec`
- `list_views`: Liste von `ListView` (nur aus Code/ddic.json)

**FieldSpec**
- `Atom`: `name`, `label`, `edm_type`, `package`, `max_length`, `precision`, `scale`, `computed` (nicht editierbar), `immutable` (nach Anlage schreibgeschützt), `value_list`
- Die Festwertlisten selbst (`value_lists`) sind eine Ebene über den Entitäten, siehe Abschnitt 2b.
- `Measure`: Betrag/Menge mit Währungs- oder Einheitsfeld (`MeasureKind::Currency` oder `Unit`)
- `Lookup`: `name`, `entity`, `collection`, `owned`

**AtomValueList** (Wertehilfe)
- `FieldValueList { list_id, prefer_dialog }`: Dropdown oder Dialog aus einer Festwertliste (Abschnitt 2b)
- `EntityRef { entity_set, key_property, display_property, filters, prefer_dialog }`: Wertehilfe aus einer anderen EntitySet, optional gefiltert über `ValueListFilter { local_property, target_property }`

**ListView**
- `name` (`default` ist die Standardansicht), `role` (heute ohne Wirkung), `columns` (Feldnamen; einwertige Navigation als `nav/feld`)

**Relationship**
- `name`, `one`, `one_field`, `many`, `many_property`, `owned` (Komposition mit Draft-Mitnahme, sonst Assoziation), `package`
- Optional `ConditionalRef` mit `Condition::SubsetOf`: beschränkt die Auswahl auf die Sammlung einer anderen Beziehung

**App (`App`)**
- `name`, `title`, `main_entity`, `subtitle`, `icon`, `package`
- `action` ist fest `display`, die URL ist `./apps/<main_entity>/`

**Package**
- `name`

## Anhang D: Umgebung der Container

| Variable | Wo gesetzt | Wert |
|---|---|---|
| `PORT` | Dockerfile | 8080 |
| `DATA_DIR` | Dockerfile | `/app/data` |
| `EXPORT_DIR` | Dockerfile | `/app/export` |
| `WEB_ROOT` | Dockerfile | `/app/web` |
| `DDIC` | compose (runtime, designer) | `/app/export/ddic.json` |
| `DESIGNER` | compose (designer) | `1` |

Volumes: `runtime-data` → `/app/data` (Runtime), `export` → `/app/export` (Designer rw, Runtime ro).

## Anhang E: Dateien mit Konfigurationscharakter

| Datei | Inhalt |
|---|---|
| `fiori-server/src/config.rs` | Alle Startparameter und Defaults |
| `compose.yaml` | Container-Setup, Ports, Volumes |
| `Dockerfile` | Image, Env-Defaults, Nutzer |
| `export/ddic.json` | Modell, vom Designer gepflegt und von der Runtime gelesen |
| `sample-data/*` | Beispielmodell (YAML-Sets, `schema.json`, `ddic.json`) |
| `fiori-server/entities.yaml` | Vermutlich Altlast, wird von keiner Datei referenziert |
