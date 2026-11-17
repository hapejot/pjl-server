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

Die Modelldaten werden in `ddic.json` gehalten. Der Designer (Abschnitt 2a) pflegt sie, die Runtime liest sie beim Start. Die Designer-Set-Abbildung steht in `fiori-server/src/ddic_store.rs`; `fiori-server/src/ddic_loader.rs` lädt das Dictionary für den Laufzeitserver.

## 2a. Der Designer

Der Designer ist kein eigenes Programm, sondern der Laufzeitserver mit `--designer` (Env `DESIGNER=1`). Er arbeitet ausschließlich auf `ddic.json`: keine Skripte, keine Datenbank, kein Entwurfszustand.

**Ablauf**

1. Der Designer öffnet `ddic.json` (`--ddic`, Standard `ddic.json`). Fehlt die Datei, wird eine leere angelegt.
2. Die Oberfläche zeigt das eincompilierte Metamodell (`DataDictionary::new()`) mit den Kern-Entitäten `Entity`, `Field`, `Relation`, `Value`, `ValueList`, `Package` und `App`. Weitere Designer-Sets bilden unter anderem Listenansichten, Facetten, UI-Konfiguration, Komponenten und Seeds ab.
3. Jede Anlage, Änderung oder Löschung wird im `DdicStore` (`fiori-server/src/ddic_store.rs`) auf die Sets abgebildet, ins Dictionary übernommen, validiert und dann atomar geschrieben (Temp-Datei `ddic.json.tmp`, danach Rename). Scheitert die Validierung, bleibt die Datei unverändert und der Request bekommt einen Konflikt.
4. Lesen greift nur auf den Speicherstand zu und schreibt nichts.

**Persistierte Sets**

Der `DdicStore` persistiert die folgenden 27 Sets. Die Pfade bezeichnen die jeweilige Abbildung in `ddic.json`; `[]` steht für ein Array-Element.

| Set | Bedeutung | Abbildung in `ddic.json` |
|---|---|---|
| `PackageSet` | Pakete | `packages[]` |
| `EntitySet` | Entitäten und Entitätsoptionen | `entities[]` |
| `FieldSet` | Atom- und Measure-Felder | `entities[].fields[]` |
| `RelationSet` | Beziehungen und Lookups | `relationships[]` |
| `AppSet` | Launchpad-Kacheln | `apps[]` |
| `ValueListSet`, `ValueSet` | Festwertlisten und ihre Werte | `value_lists[]`, `value_lists[].values[]` |
| `ComponentSet`, `ComponentUseSet`, `ComponentUseRoleSet` | Komponentendefinitionen und ihre Verwendung/Rollen | `components[]`, `component_uses[]` |
| `EntityListViewSet`, `EntityListColumnSet` | Listenansichten und Spalten | `entities[].list_views[]` |
| `EntityFilterDefaultSet` | Standardwerte der Filterleiste | `entities[].filter_defaults` |
| `EntitySelectionFieldSet` | Filterfelder | `entities[].selection_fields[]` |
| `EntityFacetSet`, `EntityFacetFieldSet` | Facetten und ihre Felder | `entities[].facets[]` |
| `EntityHiddenFieldSet`, `EntityHiddenFacetSet` | Ausgeblendete Felder und Facetten | `entities[].ui.hidden_fields[]`, `entities[].ui.hide_facets[]` |
| `EntityUiSectionSet`, `EntityUiListColumnSet`, `EntityUiListTemplateSet` | Eigene Object-Page-Abschnitte, Listenspalten und Listentabs | `entities[].ui.sections[]`, `entities[].ui.list_columns[]`, `entities[].ui.list_templates[]` |
| `FieldSideEffectSet` | Side-Effect-Ziele eines Feldes | `entities[].fields[].side_effects[]` |
| `ValueHelpFilterSet` | Kontextfilter einer `EntityRef`-Wertehilfe | `entities[].fields[].value_list.EntityRef.filters[]` |
| `AppModuleSet`, `AppControllerExtensionSet` | App-Module und Controller-Erweiterungen | `apps[].ui.modules[]`, `apps[].ui.controller_extensions[]` |
| `EntitySeedSet`, `EntitySeedValueSet` | Seed-Datensätze und ihre Feldwerte | `entities[].seed[]` |

Die festen Katalog-Sets (z. B. EDM-Typen, Prioritäten und Platzierungen) werden für die Designer-Oberfläche erzeugt und nicht in `ddic.json` gespeichert.

`ValueHelpFilterSet` bezieht sich auf Filter in `AtomValueList::EntityRef`. Die ebenfalls unterstützte Eigenschaft `Relationship.value_list_filters` wird am Relationship im Dictionary konfiguriert; dafür gibt es kein eigenes Designer-Set.

**Regeln**

- Ein Objekt braucht ein gültiges Paket (`package_ID`); unbekannte Pakete werden abgelehnt.
- Eine App braucht eine vorhandene Hauptentität (`main_entity_ID`) und ein Paket.
- Löschen einer Entität entfernt ihre Felder und Beziehungen. Löschen einer Festwertliste entfernt ihre Werte und die Zuordnung an Feldern.
- Doppelte Namen (Entität, Paket, App, Feld je Entität, Code je Festwertliste) werden abgelehnt.
- Entwürfe (Draft) gibt es im Designer nicht. Draft-Aktionen liefern den aktiven Satz zurück.
- Unbekannte Schlüssel in `ddic.json` bleiben beim Schreiben erhalten. Die Datei wird neu formatiert, Schlüssel sortiert.

**Wirksamkeit**

Die Runtime liest `ddic.json` nur beim Start. Nach Änderungen im Designer den Runtime-Server neu starten.
Das aufbereitete Dictionary wird zur Diagnose nach `DATA_DIR/active-ddic.json`
geschrieben. `DATA_DIR` muss daher auch beim PostgreSQL-Betrieb beschreibbar sein;
das Arbeitsverzeichnis des Servers muss nicht beschreibbar sein.

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
- `log("Meldung")` schreibt eine INFO-Meldung ins Server-Tracing.
  `log(ctx)` oder `log(ctx.payload)` loggt Objektstrukturen als formatiertes JSON.
  Dabei werden alle enthaltenen Felder ausgegeben, auch personenbezogene Daten;
  im produktiven Betrieb nur gezielt ausgewählte Inhalte loggen.

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

- `Dockerfile`: Setzt `PORT=8080`, `DATA_DIR=/app/data`, `WEB_ROOT=/app/web`. Standard-Webressourcen sind im Binary eingebettet; `/app/web` ist nur für zusätzliche oder überschreibende Assets vorgesehen. Läuft als Nutzer `appuser`.
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
2. **Nicht alle Dictionary-Eigenschaften haben ein eigenes Designer-Set.** Die persistierten Sets oben beschreiben den Pflegeumfang; nicht abgebildete Eigenschaften bleiben beim Schreiben aus dem Basis-JSON erhalten, sind aber nicht zwingend über die Designer-Oberfläche änderbar.
3. **Store nur bei sauberem Stopp persistiert** (siehe Abschnitt 3).
4. **Runtime lädt Modelländerungen nicht live nach.** Neustart nötig.

---

## Anhang A: Startparameter

Jede Option kann als CLI-Flag oder über die angegebene Umgebungsvariable gesetzt werden. Ein CLI-Flag hat Vorrang vor der Umgebungsvariable.

| Flag | Env | Default | Beschreibung |
|---|---|---|---|
| `--port` | `PORT` | `3034` | HTTP-Port |
| `--bind` | `BIND` | `0.0.0.0` | Lauschadresse des HTTP-Servers |
| `--data-dir` | `DATA_DIR` | `rel-data` | Verzeichnis des SQLite-Stores und der Laufzeitdateien |
| `--web-root` | `WEB_ROOT` | – (eingebettete Standardressourcen) | Optionales Verzeichnis für zusätzliche oder überschreibende Web-Assets |
| `--ddic` | `DDIC` | `ddic.json` | Dictionary-Datei |
| `--ddic-overlay` | `DDIC_OVERLAY` | – | Optionales Overlay; nur im Laufzeitmodus |
| `--schema` | `SCHEMA` | abgeleitet | Optionale feste Schemadatei (JSON) |
| `--designer` | `DESIGNER` | aus | Designer-Modus: schreibt Änderungen in `ddic.json` |
| `--auth-user-header` | `AUTH_USER_HEADER` | `X-Forwarded-User` | Benutzer-Header des Auth-Proxys |
| `--auth-required` | `AUTH_REQUIRED` | aus | Ohne Benutzer-Header mit 401 antworten |
| `--store-pg-url` | `STORE_PG_URL` | – (SQLite) | PostgreSQL-Verbindung |
| `--term3270-port` | `TERM3270_PORT` | deaktiviert | Port der optionalen 3270-Statusschnittstelle |
| `--term3270-bind` | `TERM3270_BIND` | `127.0.0.1` | Lauschadresse der 3270-Schnittstelle |
| `--scripts-dir` | `SCRIPTS_DIR` | – | Rhai-Skripte pro Set (Hooks) |
| `--ext-dir` | `EXT_DIR` | – | App-Assets, ausgeliefert unter `/ext/<app>/` |
| `--fiori-root` | `FIORI_ROOT` | – | Optionale Quelle zusätzlicher oder überschreibender Web-Assets (`file://` oder `zip://`); hat Vorrang vor `WEB_ROOT` |
| `--model-root` | `MODEL_ROOT` | – | Gemeinsame Quelle für Modell, Skripte und Extensions (`file://` oder `zip://`) |
| `--designer-root` | `DESIGNER_ROOT` | – | Designer-Dateiquelle (`file://`) |

`--designer` aktiviert den Designer, ändert aber nicht den Standardport. Für einen separaten Designer-Port muss `--port` bzw. `PORT` gesetzt werden.


## Anhang B: Zentrale Designer-Record-Felder

Die vollständigen Sets und ihre Abbildung in `ddic.json` stehen in Abschnitt 2a. Hier sind die wichtigsten Felder der Kern-Sets zusammengefasst:

**PackageSet**: `name`

Das Designer-Metamodell enthält zusätzlich `title` und `description`. Der aktuelle `DdicStore` übernimmt beim Schreiben jedoch nur `name`; Änderungen an diesen beiden Feldern werden nicht ins Dictionary zurückgeschrieben.

**EntitySet**: `name`, `plural`, `type_name`, `draft_enabled`, `audited`, `package_ID`. Im Dictionary entspricht `plural` dem `type_name_plural`.

**FieldSet**: `entity_ID`, `name`, `label`, `kind` (`Atom` oder `Measure`), `type` (EDM-Typ), `package`, `max_length`, `precision`, `scale`, `computed`, `immutable`, `multiline`, `link`, `display_format` sowie Wertehilfe-Einstellungen. Side Effects und `EntityRef`-Filter werden in Kind-Sets gespeichert.

**RelationSet**: `name`, `one_ID`, `many_ID`, `source_field`, `target_field`, `owned`, `package`, `one_label`, `many_label`.

**AppSet**: `name`, `title`, `subtitle`, `icon`, `main_entity_ID`, `package_ID`. Die Apps erscheinen als Kacheln im Launchpad (`flp.rs`, Route `/flp/`); Icons haben das Format `sap-icon://...`.

**ValueListSet**: `name`, `code_key` (Festwertliste, ID ist die Listen-UUID)

**ValueSet**: `value` (Code), `label`, `sort`, `list_ID`

`FieldSet.value_list_ID` ordnet ein Atom einer Festwertliste zu. Weitere UI-, Listen-, Seed- und Komponenten-Sets sind in der Tabelle in Abschnitt 2a aufgeführt.

Der frühere Hook `EntityDefaultsHook`, der beim Anlegen einer Entität `name` und `title` erzeugte, ist entfernt. Felder werden nur noch explizit angelegt.

## Anhang C: Dictionary-Eigenschaften

Die hier genannten Entitäts- und UI-Eigenschaften werden über die in Abschnitt 2a aufgeführten Sets abgebildet. Nicht jede Dictionary-Eigenschaft ist ein eigenes Designer-Feld; insbesondere hat `Relationship.value_list_filters` derzeit kein separates Set.

**Entity (`EntitySpec`)**
- `name`, `package`, `type_name`, `type_name_plural`, `draft_enabled`, `audited`
- `fields`: Liste von `FieldSpec`
- `list_views`: Liste von `ListView` (Designer oder `ddic.json`)
- `filter_defaults`, `selection_fields`, `facets`, `seed`

**FieldSpec**
- `Atom`: `name`, `label`, `edm_type`, `package`, `max_length`, `precision`, `scale`, `computed` (nicht editierbar), `immutable` (nach Anlage schreibgeschützt), `value_list`
- Die Festwertlisten selbst (`value_lists`) sind eine Ebene über den Entitäten, siehe Abschnitt 2b.
- `Measure`: Betrag/Menge mit Währungs- oder Einheitsfeld (`MeasureKind::Currency` oder `Unit`)
- `Lookup`: `name`, `entity`, `collection`, `owned`

**AtomValueList** (Wertehilfe)
- `FieldValueList { list_id, prefer_dialog }`: Dropdown oder Dialog aus einer Festwertliste (Abschnitt 2b)
- `EntityRef { entity_set, key_property, display_property, filters, prefer_dialog }`: Wertehilfe aus einer anderen EntitySet, optional gefiltert über `ValueListFilter { local_property, target_property }`

**ListView**
- `name` (`default` ist die Standardansicht), `role` (heute ohne Wirkung), `columns` (Liste von `ListColumn`)

**ListColumn**
- `field` (Atom oder einwertige Navigation als `nav/feld`), optional `width` (CSS: `px`, `rem`, `em`, `%`, `auto`), optional `priority` (`high`, `medium`, `low` -> `UI.Importance`)
- In `ddic.json` genügt ein blosser Feldname als String

**Relationship**
- `name`, `one`, `one_field`, `many`, `many_property`, `owned` (Komposition mit Draft-Mitnahme, sonst Assoziation), `package`
- `value_list_filters`: optionale Kontextfilter für die Wertehilfe der Einzelnavigation (`many_property`). Jeder Filter ordnet `local_property` einer Property im Ziel (`target_property`) zu.
- Die Designer-`RelationSet`-Oberfläche hat kein eigenes Feldset für `value_list_filters`; die Eigenschaft kann direkt im Dictionary gepflegt werden.
- Optional `ConditionalRef` mit `Condition::SubsetOf`: beschränkt die Auswahl auf die Sammlung einer anderen Beziehung

Beispiel: Beim Auswählen eines Ansprechpartners für eine Rechnung nur Ansprechpartner derselben Firma anbieten:

```json
{
  "name": "invoice_contacts",
  "package": "crm",
  "one": "Contact",
  "one_field": "_Invoices",
  "many": "Invoice",
  "many_property": "contact",
  "value_list_filters": [
    { "local_property": "company", "target_property": "company" }
  ]
}
```

`company` ist dabei jeweils eine einwertige Lookup-Navigation; der Server löst sie auf die zugehörige Fremdschlüssel-Property auf (z. B. `company_ID`). Alternativ können die Fremdschlüssel-Properties direkt angegeben werden.
Voraussetzung ist, dass `Invoice` und `Contact` jeweils ein entsprechendes `company`-Lookup besitzen.

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
| `WEB_ROOT` | Dockerfile | `/app/web` (optionales Overlay-Verzeichnis; Standardressourcen im Binary) |
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
