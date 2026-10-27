# Konfiguration des Fiori-Servers

Der Server hat drei Konfigurationsebenen: **Startparameter** (Port, Pfade, Auth), das **Data Dictionary** (das fachliche Modell) und die **Laufzeitdaten** (Store). Das Dictionary wird im Designer gepflegt und über einen Export als Dateien abgelegt, die der Runtime-Server liest.

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

Der Designer zeigt ein eincompiliertes Metamodell (`DataDictionary::new()`) mit den Entities `Entity`, `Field`, `Relation`, `Value`, `Package`, `App`. Seine Daten liegen in `ddic.json` und werden über `DdicStore` (`fiori-server/src/ddic_store.rs`) abgebildet: Die Sets `PackageSet`, `EntitySet`, `FieldSet`, `RelationSet`, `AppSet` werden gelesen und geschrieben, andere Sets bleiben flüchtig. Jede Schreiboperation wird validiert und atomar in die Datei geschrieben. Unbekannte Schlüssel in `ddic.json` bleiben erhalten.

Die Modelldaten selbst liegen als Sets im Store: `PackageSet`, `EntitySet`, `FieldSet`, `RelationSet`, `AppSet`. Der Loader (`fiori-server/src/ddic_loader.rs`) baut daraus das Dictionary. Die Felder stehen in Anhang B.

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

- **Designer-Schreibvorgänge** sind transaktional: Ungültige Dictionaries werden abgelehnt, die Datei bleibt dann unverändert.
- **Runtime** lädt `ddic.json` nur beim Start. Nach Änderungen im Designer muss der Runtime-Server neu gestartet werden. Ohne Dictionary bricht er mit einem Hinweis ab.
- **Schema**: Ohne `--schema` wird das Schema aus dem Dictionary abgeleitet, inklusive Migration neuer Tabellen. Mit `--schema` wird eine feste Schemadatei verwendet.
- **Store-Backend**: Standard ist SQLite unter `DATA_DIR`. Mit `STORE_PG_URL` läuft der Store in PostgreSQL.

### Persistenz-Hinweis

Der SQLite-Store läuft im Speicher und wird nur beim sauberen Beenden (SIGINT/SIGTERM) auf die Platte geschrieben (`server.rs`). Stürzt der Prozess ab, gehen Änderungen seit dem letzten Stopp verloren. Bei PostgreSQL entfällt dieses Problem.

## 4. Docker / Compose

- `Dockerfile`: Setzt `PORT=8080`, `DATA_DIR=/app/data`, `EXPORT_DIR=/app/export`, `WEB_ROOT=/app/web`. Läuft als Nutzer `appuser`.
- `compose.yaml`:
  - Designer: Port `3033:8080`, Env `DESIGNER=1`, Volume `export` (schreibbar).
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

Im Designer wird beim Anlegen einer Entity über den Hook `EntityDefaultsHook` (`hook.rs`) automatisch ein Feldpaar `name` und `title` erzeugt.

## Anhang C: Dictionary-Eigenschaften

Nicht alles davon ist im Designer-Store abbildbar (siehe Auffälligkeit 2).

**Entity (`EntitySpec`)**
- `name`, `package`, `type_name_plural`
- `fields`: Liste von `FieldSpec`
- `list_views`: Liste von `ListView` (nur aus Code/ddic.json)

**FieldSpec**
- `Atom`: `name`, `label`, `edm_type`, `package`, `max_length`, `precision`, `scale`, `computed` (nicht editierbar), `immutable` (nach Anlage schreibgeschützt), `value_list`
- `Measure`: Betrag/Menge mit Währungs- oder Einheitsfeld (`MeasureKind::Currency` oder `Unit`)
- `Lookup`: `name`, `entity`, `collection`, `owned`

**AtomValueList** (Wertehilfe)
- `FieldValueList { list_id, prefer_dialog }`: Dropdown oder Dialog aus einer Werteliste
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
| `DDIC` | compose (runtime) | `/app/export/ddic.json` |

Volumes: `runtime-data` → `/app/data` (Runtime), `export` → `/app/export` (Designer rw, Runtime ro).

## Anhang E: Dateien mit Konfigurationscharakter

| Datei | Inhalt |
|---|---|
| `fiori-server/src/config.rs` | Alle Startparameter und Defaults |
| `compose.yaml` | Container-Setup, Ports, Volumes |
| `Dockerfile` | Image, Env-Defaults, Nutzer |
| `export/ddic.json`, `export/schema.json` | Exportierter Stand für die Runtime |
| `sample-data/*` | Beispielmodell (YAML-Sets, `schema.json`, `ddic.json`) |
| `fiori-server/entities.yaml` | Vermutlich Altlast, wird von keiner Datei referenziert |
