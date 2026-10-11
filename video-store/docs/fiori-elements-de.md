# video-store: Von der Postgres-Datenbank zur Fiori-Elements-Oberfläche

Stand: aus dem Quellcode abgeleitet (`video-store/`, `spec/`, `spec-schema/`, `fiori-server/`).
Dieses Dokument beschreibt den **Ist-Zustand** und die **vorhandenen Erweiterungspunkte**.
Neue Erweiterungen werden später separat definiert.

---

## 1. Überblick

Die Video-Datenbank (Postgres) wird ohne handgeschriebene Modelle, ohne Code-Generierung
für die UI und ohne eigene UI-Anwendung in eine laufende Fiori-Elements-App überführt.
Die Oberfläche entsteht vollständig aus Metadaten:

```
Postgres-Katalog
   │  video-ddic  (introspect + map::build_model)
   ▼
ddic.json  ── Data Dictionary (Packages, Entities, Relationships, Apps)
   │
   ├── video-import  (Zeilen -> RelationalStore / SQLite)
   │
   ▼
pjl-runtime --ddic ddic.json --data-dir <dir>
   ├── /Service/$metadata     EDMX mit Annotationen (UI.*, Common.*, Capabilities)
   ├── /Service/<Set>         OData-V4-CRUD inkl. Drafts
   ├── /apps/Video/manifest.json   Fiori-Elements-Manifest (ListReport + ObjectPage)
   ├── /apps/Video/Component.js
   └── /flp/ , /              Launchpad (Shell-Start, Kachel "Videos")
```

Die Kette ist in zwei Phasen aufgeteilt (`video-store/docker/migrate.sh`, `compose.yaml`):

| Phase | Dienst | Aufgabe |
|---|---|---|
| Migration (one-shot) | `migrate` | `video-ddic` schreibt `ddic.json`, `video-import` schreibt die Daten in den SQLite-Store. Idempotent über `.migrated`-Marker. |
| Laufzeit (dauerhaft) | `runtime` | `pjl-runtime` liest nur `ddic.json` und den Store, liefert OData und die Fiori-App. |

Die Quelldatenbank wird nur lesend benutzt.

---

## 2. Schritt 1: Katalog lesen (`video-store/src/introspect.rs`)

`video-ddic` liest über `tokio-postgres` den Katalog: Tabellen, Spalten (`udt`, `nullable`,
Länge, Precision/Scale), Primärschlüssel, Fremdschlüssel inklusive `ON DELETE`-Regel.
Ergebnis ist ein `PgCatalog`.

Die Datei `video-store/ddic.json` (gitignored) ist das Ergebnis; sie enthält aktuell
**34 Entitäten, 33 Relationships, 1 App** im Package `video`.

---

## 3. Schritt 2: Abbildung auf das Data Dictionary (`video-store/src/map.rs`)

`map::build_model` ist die **einzige Quelle der Wahrheit**. `ddic.json` und der Import
verwenden dieselben Regeln.

### 3.1 Tabelle -> Entität

| Regel | Beispiel |
|---|---|
| Entität = Singular in PascalCase (`singular()` + `pascal()`) | `videos` -> `Video` |
| Entity-Set = `{Entität}Set` | `VideoSet` |
| Paket = `video` (`PACKAGE`) | alle Entitäten |
| Singularisierung nur am letzten Segment | `label_categories` -> `LabelCategory`, `faces` -> `Face`, `emb_dinov2` bleibt |

### 3.2 Spalten -> Felder (Atoms)

Typabbildung in `atom_for()`:

| Postgres (`udt`) | EDM-Typ |
|---|---|
| `text`, `varchar`, `bpchar`, `name` | `Edm.String` (mit `MaxLength`) |
| `int2`, `int4` / `int8` | `Edm.Int32` / `Edm.Int64` |
| `float4`, `float8` | `Edm.Double` |
| `numeric` | `Edm.Decimal` (Precision/Scale) |
| `bool` | `Edm.Boolean` |
| `uuid` | `Edm.Guid` |
| `timestamp(tz)` | `Edm.DateTimeOffset` |
| `date` | `Edm.Date` |
| `bytea` | `Edm.Binary` (Quelle Hex-Text, Ablage Base64) |
| `jsonb`, `json`, Enums, `vector(n)` | `Edm.String` (Text, `TODO(pg-backend)`) |

### 3.3 Primärschlüssel -> `ID`

| `KeyMode` | Bedingung | Ergebnis |
|---|---|---|
| `Uuid` | einzelner `uuid`-PK | `ID` = Quell-ID unverändert |
| `Legacy` | einzelner Nicht-UUID-PK | `ID` = UUIDv5(Tabelle, Schlüssel), Original als Feld `legacy_id` |
| `Synth` | zusammengesetzt oder fehlend | `ID` = UUIDv5 über alle PK-Werte bzw. die ganze Zeile |

Die UUIDv5-Namespace-Konstante `KEY_NAMESPACE` macht den Import wiederholbar ohne Mapping-Tabelle.

### 3.4 Name-Feld (`name`)

Jede Entität bekommt ein Feld `name` (Titel im Object Page Header, `UI.HeaderInfo`):

1. vorhandene Spalte `name` hat Vorrang,
2. sonst Eintrag in `NAME_SOURCE`: `videos.file_name`, `scenes.title`,
3. sonst wird `name` aus den Schlüsselspalten (bzw. FK-Spalten) abgeleitet.

### 3.5 Fremdschlüssel -> Relationships und Navigation

Jede FK-Beziehung wird zu einer `Relationship` mit Namen `{kind}_{spalte}`, z. B.
`video_tags_video_id`. Daraus entstehen:

* auf der Kindseite: Navigation ohne `_`, Name aus `nav_name()`: `video_id` -> `video`
  (Feld `video_ID` als Speicherspalte),
* auf der Elternseite: Sammelnavigation `collection_nav()`: `_VideoTags`.

**Komposition vs. Assoziation** (`choose_compositions()`):

* Komposition, wenn die Constraint `ON DELETE CASCADE` ist **und** die Spalte `NOT NULL` ist.
  Pro Kind höchstens eine Komposition (das Schema erlaubt nur einen Elternteil).
* Sonst Assoziation.
* Overrides: `FORCE_COMPOSITION` (`video_jobs.agent_id`), `FORCE_ASSOCIATION` (`video_jobs.video_id`).

**Implizite FKs** ohne DB-Constraint stehen fest in `IMPLICIT_FKS` (z. B. `videos.scene_id -> scenes`,
`emb_images.video_id -> videos`, `actor_tag_map.tag_id -> actor_tags`).

### 3.6 Ergebnis-Beispiele aus `ddic.json`

| Relationship | Eltern -> Kind | Kind-Rolle |
|---|---|---|
| `videos_agent_id` | Agent -> Video | Komposition |
| `video_tags_video_id` | Video -> VideoTag | Komposition |
| `video_tags_tag_id` | Tag -> VideoTag | Assoziation |
| `video_jobs_video_id` | Video -> VideoJob | Assoziation (Override) |
| `videos_scene_id` | Scene -> Video | Assoziation (implizit) |

---

## 4. Schritt 3: App-Definition

In `build_model` wird genau eine App angelegt:

```rust
App::new("videos", "Videos", "Video", Some("List Videos"), None, Package::new("video"))
```

Die App ist an die Hauptentität `Video` gebunden. Der Name `videos` erscheint als
`sap.app.id`, der Titel als Kachel im Launchpad.

Die App wird in `fiori-server/src/flp.rs` über `ddic.apps()` gefunden. Das Manifest
wird **zur Laufzeit** pro Request erzeugt (`app_manifest_handler`), nicht gespeichert.

---

## 5. Schritt 4: Speicherschema und Import

* `Model::schema()` leitet über `spec-schema::schema_from_ddic` das Speicherschema ab.
  Schlägt das fehl, wird das Modell **nicht** erzeugt (`build_model` validiert am Ende).
* `video-import` legt einen **frischen** `RelationalStore` (`sqlite-relational-store`) an
  und schreibt die Zeilen Parent-vor-Kind (`import::records_for_table`).
* Der Zielordner muss leer sein; ein zweiter Lauf scheitert an den Primärschlüsseln.

Verlorene Informationen (nicht im DDIC abbildbar, laut `lib.rs`): `NOT NULL`, `UNIQUE`,
`CHECK`, Performance-Indizes, Views (`tag_stats`, `video_stats`, ...), `ON DELETE SET NULL`
als Kaskade, Eindeutigkeit bei zusammengesetzten Schlüsseln.

---

## 6. Schritt 5: Ableitung der Fiori-Elements-Oberfläche

Die Oberfläche besteht aus drei Schichten, die alle aus dem DDIC generiert werden.

### 6.1 OData-Service und Annotationen (`fiori-server/src/srv/mod.rs`)

`$metadata` wird als EDMX erzeugt. Für jede Entität gibt es:

| Annotation | Quelle / Inhalt |
|---|---|
| `UI.HeaderInfo` | `TypeName` = Entität, `TypeNamePlural` = Set, `Title` = Feld `name` |
| `UI.LineItem` (Liste) | **nur** die Spalte `name` (siehe Abschnitt 7) |
| `UI.Facets` | Abschnitt "General" (-> `FieldGroup#Main`) + je ein `ReferenceFacet` pro Sammelnavigation (`{nav}/@UI.LineItem`) |
| `UI.FieldGroup#Main` | `UI.DataField` für jedes Feld; FK-Felder als `{nav}_ID` |
| `Common.Label` | pro Feld `label` = Spaltenname (z. B. `file_name`) |
| `Common.SemanticKey` | `ID` |
| `Common.Messages` | `DraftMessages` |
| `Common.DraftRoot` / `Common.DraftNode` | Draft-Root für Entitäten ohne Parent, sonst `DraftNode` |
| `Org.OData.Core.V1.Computed` | `ID`, `IsActiveEntity`, `HasActiveEntity`, `HasDraftEntity` |
| `Org.OData.Capabilities.V1.*` | Insert/Update/Delete-Restriktionen, `NavigationRestrictions` |
| `Common.Text` + `UI.TextArrangement` | bei Fremdschlüsseln mit Anzeigewert |
| `Measures.ISOCurrency` / `Unit` | bei `FieldSpec::Measure` |
| `Common.ValueList` | nur bei `AtomValueList` gesetzt (im Video-Modell nicht gesetzt) |

Die Draft-Handhabung (`draftActivate`, `draftEdit`, `draftPrepare`) und die Schema-Vokabulare
sind in `docs/draft-de.md` beschrieben.

### 6.2 Manifest und Routing (`fiori-server/src/flp.rs`, `app_manifest_handler`)

Das Manifest ist eine Fiori-Elements-V4-App mit Flexible Column Layout:

* `sap.fe.templates.ListReport` für `VideoList`, `entitySet: VideoSet`,
  `initialLoad: Enabled`, `variantManagement: Page`.
* `sap.fe.templates.ObjectPage` für `VideoObjectPage` (Mid-Column), `contextPath: /VideoSet`.
* Pro Sammelnavigation eine weitere ObjectPage (End-Column) mit eigener Route,
  z. B. `VideoVideoTagsObjectPage`, Pattern `VideoSet({key})/_VideoTags({key2})`.
* Routen: `:?query:` (Liste), `VideoSet({key}):?query:` (Objekt), darunter die Kind-Routen.
* `dataSources.mainService`: `/Service/`, OData 4.0, `autoExpandSelect`, `earlyRequests`.
* `sap.app.crossNavigation`: semantisches Objekt `Video`, Aktion `display`.
* `_version` 1.65.0, `minUI5Version` 1.139.0.

### 6.3 Launchpad und Startseite (`flp.rs`, `static/flp/init.js`, `templates/info.hbs`)

* `/` liefert die Startseite; `/flp/site.json` (`flp.rs:43`) beschreibt Kacheln und Inbounds.
* Die Kachel für `videos` verweist auf den Intent `Video-display`.
* `/apps/{app}/Component.js` liefert `sap.fe.core.AppComponent` mit `metadata.manifest: "json"`.
* Statische Assets liegen unter `WEB_ROOT` (`web/`), Fallback über `ServeDir`.

---

### 6.4 Listenansichten (`ListView`)

Die Spalten der Liste sind explizit und benannt an der Entität definiert:

* `EntitySpec.list_views: Vec<ListView>` mit `name`, optional `role` (noch ohne Wirkung), geordneten `columns`.
* Spalten sind Atome (`file_size`) oder einwertige Navigationen (`scene` -> Anzeige `scene/name`).
  Sammelnavigationen sind nicht zulässig.
* `default` ist die Standardansicht (unqualifiziertes `UI.LineItem`). Jede weitere Ansicht
  erzeugt `UI.LineItem#<name>` und `UI.PresentationVariant#<name>`.
* Mehr als eine Ansicht: ListReport zeigt eine View-Leiste (`options.settings.views.paths`).
* Ohne Ansichten bleibt das frühere Verhalten (nur `name`).
* Validierung: `DataDictionary::validate_list_views()`, aufgerufen beim Bauen des Video-Modells
  und beim Laden von `ddic.json` im Runtime-Server.
* Pflege für den Video-Store in `map.rs`:

  ```rust
  pub const LIST_VIEWS: &[(&str, &str, &[&str])] = &[(
      "videos", "default", &["name", "file_size", "scene", "agent"],
  )];
  ```

## 7. Beobachtete Lücken im Ist-Zustand

Diese Punkte sind im Code belegt. Sie sind **keine Fehler in der Dokumentation**, sondern
Stellen, an denen die Erzeugung nicht so weit geht wie die Typdefinitionen vermuten lassen.

| # | Befund | Beleg |
|---|---|---|
| L1 | ~~Die Listenansicht zeigt nur die Spalte `name`.~~ **Behoben:** explizite Listenansichten (`EntitySpec.list_views`, Konstante `LIST_VIEWS` in `map.rs`). Siehe Abschnitt 6.4. | `spec/src/entity_spec.rs` `ListView`, `srv/mod.rs` `line_item_annotations` |
| L2 | ~~`PresentationOverrides` wurde nicht ausgewertet.~~ **Entfernt:** das Konstrukt `PresentationOverrides` samt Feld `presentation` an Atomen und Measures ist abgeschafft. Listenspalten werden über `EntitySpec.list_views` gesteuert (Abschnitt 6.4). | `spec/src/entity_spec.rs` |
| L3 | Labels sind die rohen Spaltennamen (`file_name`, `agent_id`), keine lesbaren Texte. | `map.rs` `atom_field()`: `label: atom.name` |
| L4 | `_`-Präfix soll laut Doku die TableFacet-Generierung unterdrücken. Der Code in `flp.rs` prüft das nicht; alle to-many Navigationen bekommen eine Kind-Route. | `spec/src/relationship.rs:9` vs. `flp.rs` ca. Zeile 303-308 |
| L5 | `FacetSectionSpec` und `TableFacetSpec` existieren als Typen, werden aber in `fiori-server` nicht verwendet. | `spec/src/entity_spec.rs:408-420` |
| L6 | `video-store` setzt für keine Spalte eine `AtomValueList`. Wertehilfen entstehen nur über Fremdschlüssel (Lookups). | `map.rs` `atom_field()`: `value_list: None` |
| L7 | Die Annotation-Erzeugung für `FieldSpec::Measure` ist mit `todo!()` in `prepare_field_group` markiert. Im Video-Modell gibt es keine Measures, daher kein Laufzeitfehler. | `srv/mod.rs` `prepare_field_group` |
| L8 | `sample.rs` enthält ein XML-Annotations-Beispiel; es wird nicht aufgerufen (auskommentiert in `srv/mod.rs` ca. Zeile 2125). | `srv/sample.rs`, `srv/mod.rs:2125` |

Im Designer-Modus (`pjl-designer`) gibt es zusätzlich einen Button "Publish Dictionary"
(`flp.rs` `list_control_configuration`). Im Runtime-Modus (`video-store`) ist dieser nicht vorhanden.

---

## 8. Erweiterungsmöglichkeiten

Die Erweiterungen sind nach Ebene geordnet. Jede Ebene hat einen klaren Ansatzpunkt.

### 8.1 Migrations-Ebene (`video-store/src/map.rs`)

Diese Konstanten sind die direkten Stellschrauben:

| Konstante / Funktion | Wirkung |
|---|---|
| `PACKAGE` | Paketname des gesamten Modells |
| `KEY_NAMESPACE` | UUID-Namensraum; **nicht ändern**, sonst ändern sich alle abgeleiteten IDs |
| `IMPLICIT_FKS` | Fehlende Fremdschlüssel ergänzen (Tripel Kind, Spalte, Eltern) |
| `FORCE_COMPOSITION` | Komposition erzwingen, obwohl die DB keine CASCADE-Regel hat |
| `FORCE_ASSOCIATION` | Komposition verbieten, obwohl die DB CASCADE hat |
| `NAME_SOURCE` | Quellspalte für das Feld `name` festlegen |
| `atom_for()` | Typabbildung Postgres -> EDM ändern oder erweitern |
| `singular()`, `entity_name()` | Benennung der Entitäten und Sets |
| `is_reserved()` | Feldnamen, die nicht verwendet werden dürfen |

Änderungen wirken nach `video-ddic` + `video-import` (`migrate.sh --force`).

### 8.2 Modell-Ebene (`spec`, DDIC)

* **Entitäten und Felder**: `EntitySpec::new(...).field(...)`. Feldtypen über `FieldSpec::Atom`,
  `FieldSpec::Measure` (Währung/Einheit), `FieldSpec::Lookup`.
* **Relationships**: `Relationship::new(...).with_one(...).with_many(...).owned()` steuert
  Komposition und damit die Kind-Routen im Manifest.
* **Apps**: `App::new(name, title, main_entity, subtitle, icon, package)`. Mehrere Apps sind
  möglich, `build_model` legt heute nur eine an.
* **Listenansichten** (`ListView`): Spalten und Reihenfolge je Ansicht, siehe 6.4.
  Suche, Sortierung und Gruppen sind noch nicht modelliert; künftige Metadaten
  gehören als eigene, explizite Konstrukte an die Entität, nicht als Feld-Overrides.
* **Wertehilfen** (`AtomValueList::EntityRef`, `FieldValueList`): erzeugen `Common.ValueList`.
  Video-Modell setzt sie nicht; die Erweiterung wäre eine Zuordnung in `atom_field()`.

### 8.3 Annotations-Ebene (`fiori-server/src/srv/mod.rs`)

Hier liegt die größte Gestaltungsfreiheit für die UI:

* `UI.LineItem` für Listenspalten (heute nur `name`),
* `UI.SelectionFields` für Filterfelder (heute nicht erzeugt),
* `UI.SortOrder` über ein neues Modellkonstrukt für Standardsortierung,
* `UI.Facets` und `UI.FieldGroup` für die Object-Page-Struktur,
* `UI.DataFieldForAction` für gebundene Aktionen (Draft-Aktionen sind bereits vorhanden),
* `UI.Criticality` (Statusfarben), sobald ein Modellkonstrukt dafür existiert,
* `Common.Label` für lesbare Beschriftungen (L3).

Der Einstieg ist jeweils `Annotations::from_entity` bzw. `from_field`.

### 8.4 Manifest-Ebene (`fiori-server/src/flp.rs`)

* `app_manifest_handler`: Routen, Targets, Modelleinstellungen (`autoExpandSelect`,
  `earlyRequests`, `operationMode`), `variantManagement`, `initialLoad`.
* `list_control_configuration`: Toolbar-Aktionen und Controls der Liste über
  `controlConfiguration` (heute nur im Designer genutzt).
* `Component.js`: Komponenten-Erweiterungen. Die Datei wird serverseitig erzeugt.
* `ext/Publish.js`: Beispiel für eine Custom-Action, die als Modul geladen wird.

### 8.5 Laufzeit- und Daten-Ebene (`fiori-server/src/hook.rs`)

* `Hook`-Trait: Vor- und Nachverarbeitung der Operationen `Create`, `Patch`, `Read`,
  `Delete`, `DraftEdit`, `DraftActivate`, `DraftPrepare`. Eingehängt über `Extender::new`.
* `EntityDefaultsHook` ist ein Beispiel (Designer-Modus, setzt Standardfelder).
  Für die Video-Runtime ist derzeit kein Hook eingehängt (`server.rs` ca. Zeile 130).
* Ein eigener `DataStore` kann den `RelationalStore` ersetzen (Trait `DraftDataStore`).

### 8.6 Konfiguration und Deployment (`compose.yaml`, `Dockerfile`)

* Umgebungsvariablen: `PORT`, `DATA_DIR`, `EXPORT_DIR`, `WEB_ROOT`, `DDIC`, `RUST_LOG`,
  zusätzlich für die Video-Erweiterung `THUMBNAIL_BASE_URL` (Default `http://nuc:9000/proxy`).
* Authentifizierung: `AUTH_USER_HEADER` (Standard `X-Forwarded-User`) und `AUTH_REQUIRED`
  (Standard aus). Details in 10.12.
* Das Runtime-Image startet `/app/video-runtime` (nicht mehr `pjl-runtime`). `video-runtime`
  ist `pjl-runtime` plus die Video-Erweiterungen aus Abschnitt 10.
* `WEB_ROOT` für eigene statische Assets. Liegt dort eine App-Datei, wird sie über den
  Fallback-Dienst ausgeliefert.
* Migration erneut auslösen: `docker compose ... run --rm migrate --force`.

### 8.7 Zusätzliche Apps und Einstiegspunkte

* Weitere Apps: zusätzliches `App::new` in `build_model`. Jede App braucht eine Hauptentität;
  Routen und Kachel entstehen automatisch.
* Anderer Einstieg: `main_entity` bestimmt die Liste; eine Entität ohne `Sammelnavigation`
  zeigt keine Unterseiten.

### 8.8 Erweiterungen (Extension-Interface)

Ab hier ist die eigentliche Erweiterungsstelle das Trait `Extension` aus
`fiori-server/src/extension.rs`. Der fiori-server kennt keine konkrete Erweiterung;
Abhängigkeiten zeigen nur von der Erweiterung zum Server, nie umgekehrt.
Vollständiges Beispiel: Abschnitt 10.

---

## 9. Kurzreferenz: Dateien

| Datei | Verantwortung |
|---|---|
| `video-store/src/introspect.rs` | Postgres-Katalog lesen |
| `video-store/src/map.rs` | Abbildung auf DDIC, Regeln, Konstanten |
| `video-store/src/import.rs` | Zeilen in den Store schreiben |
| `video-store/src/bin/video-ddic.rs` | Erzeugt `ddic.json` |
| `video-store/src/bin/video-import.rs` | Importiert die Daten |
| `video-store/docker/migrate.sh` | Migrationsablauf, Marker |
| `video-store/compose.yaml`, `Dockerfile` | Zwei Dienste: `migrate` und `runtime` |
| `spec/src/entity_spec.rs` | Felder, Listenansichten, Wertehilfen |
| `spec/src/relationship.rs` | Beziehungen, Komposition |
| `spec/src/app.rs` | App-Definition |
| `spec-schema` | Speicherschema aus dem DDIC |
| `fiori-server/src/srv/mod.rs` | EDMX, Annotationen, OData-Service |
| `fiori-server/src/flp.rs` | Manifest, Routen, Launchpad, Component |
| `fiori-server/src/hook.rs` | Hooks für Laufzeitlogik |
| `fiori-server/src/server.rs` | Routing und Start, `run_with(config, extensions)` |
| `fiori-server/src/extension.rs` | Trait `Extension`, Zusammenführen der Object-Page-Beiträge |
| `video-store/src/bin/video-runtime.rs` | Startet den Server mit den Video-Erweiterungen |
| `video-store/src/thumbnails.rs` | Video-Erweiterung: Proxy, Custom Section, i18n-Routen |
| `video-store/fragments/Thumbnails.fragment.xml` | Fragment der Custom Section |
| `video-store/i18n/*.properties` | Sprachdateien (Standard Deutsch, `_en`, `_de`) |
| `docs/draft-de.md` | Draft-Verhalten (Spezifikation) |

---

## 10. Erweiterungen selbst bauen (Anleitung)

Diese Anleitung beschreibt, wie du eine neue Erweiterung für die Video-Oberfläche baust,
ohne den fiori-server zu verändern. Alle Beispiele stammen aus dem Code, der heute im Repo liegt.

### 10.0 Grundregel

* `fiori-server` kennt **keine** Fachentität (kein `Video`, kein `Agent`). Erweiterungen hängen
  sich über das Trait `Extension` ein. Abhängigkeiten zeigen nur **von** der Erweiterung **zum** Server.
* Fachlogik (Entitätsnamen, Bildquellen, Fragmente, Sprachtexte) gehört in den `video-store`
  bzw. in ein eigenes Crate, das `fiori-server` als Bibliothek nutzt.

### 10.1 Was das Trait kann

Datei `fiori-server/src/extension.rs`:

```rust
pub trait Extension: Send + Sync {
    /// Zusätzliche HTTP-Routen. Bekommt den Server-Zustand (Store, Konfiguration).
    fn routes(&self, state: AppState) -> Router { Router::new() }

    /// UI5-Modulpfade als (Namespace, URL). Werden in Component.js registriert.
    /// ("video.ext", "/ext/video") bildet Module "video/ext/..." auf "/ext/video/..." ab.
    fn ui5_paths(&self) -> Vec<(String, String)> { Vec::new() }

    /// Beitrag zu content.body.sections der Object Page der Hauptentität `entity`.
    fn object_page_content(&self, entity: &str) -> Option<Value> { None }

    /// Zusätzliche Spalten der Listen-Tabelle der Hauptentität `entity`.
    fn list_columns(&self, entity: &str) -> Option<Value> { None }
}
```

Mehrere Erweiterungen werden kombiniert: Routen werden gemerged, Sections und Spalten
zusammengeführt. Bei gleichem Schlüssel gewinnt die später eingehängte Erweiterung.

### 10.2 Einhängen

`video-store/src/bin/video-runtime.rs`:

```rust
let extensions: fiori_server::extension::Extensions = vec![
    Arc::new(VideoThumbnails::from_env()),
    Arc::new(MeineErweiterung::new()),          // neu hinzufügen
];
server::run_with(config, extensions).await
```

Die Erweiterung braucht ein `pub mod` in `video-store/src/lib.rs`, damit das Binary sie sieht.

### 10.3 Rezept A: Eigene Route (z. B. Proxy oder Datenendpoint)

```rust
use std::sync::Arc;
use axum::{extract::{Path, State}, http::{header, StatusCode}, response::{IntoResponse, Response},
           routing::get, Router};
use data_store::EntityKey;
use fiori_server::{extension::Extension, AppState};
use odata::query::ODataQuery;

struct Meine { app: AppState }

impl Extension for Meine {
    fn routes(&self, app: AppState) -> Router {
        Router::new()
            .route("/ext/meine/hallo/{id}", get(hallo))
            .with_state(Arc::new(app))
    }
}

async fn hallo(State(app): State<Arc<AppState>>, Path(id): Path<String>)
    -> Result<Response, StatusCode>
{
    let uuid = uuid::Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    // Aktive Version lesen (true), sonst Draft (false):
    let rec = app.store
        .read_entity("VideoSet", &EntityKey::new(uuid, true), &ODataQuery::default())
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let name = rec.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], name).into_response())
}
```

Die Methode `read_entity` steht im Trait `DataStore` (Supertrait von `DraftDataStore`).
Ein Lesen über `$expand` im Draft liefert unvollständige Daten (siehe 10.9). Deshalb direkt lesen.

### 10.4 Rezept B: Custom Section auf der Object Page

Ziel: eine neue Section, die eine Standard-Facet ersetzt.

1. **Section liefern** (`object_page_content`):

```rust
fn object_page_content(&self, entity: &str) -> Option<Value> {
    if entity != "Video" { return None; }
    Some(json!({ "body": { "sections": {
        "_VideoThumbnailsSection": { "visible": false },   // Standard-Facet ausblenden
        "VideoThumbnails": {
            "template": "video.ext.fragment.Thumbnails",
            "title": "{{thumbnailsTitle}}",
            "position": { "placement": "After", "anchor": "GeneralSection" }
        }
    }}}))
}
```

   * Der Schlüssel der Standard-Facet ist `<Navigation>Section` mit dem Navigationsnamen,
     z. B. `_VideoThumbnailsSection`. Konvention aus `srv/mod.rs` (`list_facet_records`).
   * `position.anchor` verweist auf die ID einer bestehenden Section, hier `GeneralSection`.

2. **Modulpfad registrieren** (`ui5_paths`):

```rust
fn ui5_paths(&self) -> Vec<(String, String)> {
    vec![("video.ext".into(), "/ext/video".into())]
}
```

3. **Fragment ausliefern** (Datei `video-store/fragments/Thumbnails.fragment.xml`, per
   `include_str!` eingebettet, Route in `routes()`):

```rust
const FRAGMENT: &str = include_str!("../fragments/Thumbnails.fragment.xml");

// in routes():
.route("/ext/video/fragment/Thumbnails.fragment.xml", get(fragment))

async fn fragment() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/xml; charset=utf-8")], FRAGMENT)
}
```

   Der Pfad ergibt sich aus dem Template: `video.ext.fragment.Thumbnails` wird zu
   `/ext/video/fragment/Thumbnails.fragment.xml`. Ein Abweichen führt zu einem 404 im Browser.

4. **Fragment schreiben** (Vorlage, siehe `fragments/Thumbnails.fragment.xml`):

```xml
<core:FragmentDefinition xmlns="sap.m" xmlns:core="sap.ui.core">
  <FlexBox wrap="Wrap" items="{ path: '_VideoThumbnails',
                               parameters: { $orderby: 'thumbnail_index' },
                               templateShareable: false }">
    <Image src="{= '/ext/video/thumbnail/' + ${video_ID} + '/' + ${thumbnail_index} }"
           width="200px" densityAware="false" class="sapUiTinyMargin" />
  </FlexBox>
</core:FragmentDefinition>
```

   * Das Fragment ist an das Video gebunden. Relative Pfade wie `_VideoThumbnails` funktionieren.
   * Fremdschlüssel direkt lesen (`video_ID`), nicht über `video/...` mit `$expand` (10.9).
   * Das Template hat **keinen** eigenen Controller. Für Logik in JavaScript bräuchte es eine
     eigene Datei unter dem Namespace; bisher nicht nötig.

### 10.5 Rezept C: Zusätzliche Listenspalte

`list_columns("Video")` liefert die Spalte, `template` zeigt auf ein Zellen-Fragment:

```rust
fn list_columns(&self, entity: &str) -> Option<Value> {
    if entity != "Video" { return None; }
    Some(json!({
        "VideoPreview": {
            "header": "{{previewColumnTitle}}",
            "width": "120px",
            "horizontalAlign": "Begin",
            "position": { "placement": "Before", "anchor": "name" },
            "template": "video.ext.fragment.PreviewColumn"
        }
    }))
}
```

Zellen-Fragment `fragments/PreviewColumn.fragment.xml`:

```xml
<core:FragmentDefinition xmlns="sap.m" xmlns:core="sap.ui.core">
  <Image src="{= '/ext/video/thumbnail/' + ${ID} + '/0' }" width="96px"
         densityAware="false" class="sapUiTinyMarginTopBottom" />
</core:FragmentDefinition>
```

* `${ID}` ist die Video-ID der Zeile. Eine zusätzliche Navigation ist nicht nötig.
* Der fiori-server führt die Spalten über `list_control_configuration_with_extensions` in
  `@com.sap.vocabularies.UI.v1.LineItem.columns` ein.
* `anchor` verweist auf den Schlüssel einer bestehenden Spalte (hier `name`).

### 10.6 Sprachdateien (i18n)

* Dateien in `video-store/i18n/`: `i18n.properties` (Standard, Deutsch), `i18n_en.properties`,
  `i18n_de.properties`. Jede enthält dieselben Schlüssel.
* Ausliefern über feste Routen unter `/apps/Video/i18n/...` (siehe `thumbnails.rs`, Funktionen
  `i18n_default`, `i18n_de`, `i18n_en`).
* Verwendung in Manifest-Werten als `{{schluessel}}` (Section-Titel, Spalten-Header).
* Neuer Schlüssel: in **alle drei** Dateien eintragen, sonst zeigt die Oberfläche den Schlüssel.

### 10.7 Entwicklungszyklus

1. Rust-Code und Fragmente ändern (Fragmente werden per `include_str!` eingebettet).
2. Tests: `cargo test -p video-store` und `cargo check --workspace`.
3. Image bauen und Runtime neu starten:

   ```
   docker compose -f video-store/compose.yaml up --build -d runtime
   ```

4. Im Browser hart neu laden (Strg+F5). UI5 cacht Module und Preload-Dateien.
5. Browser-Konsole prüfen: 404 auf Fragment- oder i18n-Pfade bedeuten, dass Namespace-Mapping
   oder Route nicht zusammenpassen.
6. Bei Datenproblemen Migration neu laufen lassen: `migrate --force` (siehe Abschnitt 8.6).

### 10.8 Checkliste für eine neue Erweiterung

- [ ] Ziel klar: Route, Section, Spalte, Sprachtext oder Kombination
- [ ] Datei im `video-store` (nicht im `fiori-server`)
- [ ] Trait-Methoden implementiert, die tatsächlich gebraucht werden
- [ ] In `video-runtime.rs` eingehängt
- [ ] Fragment-Route entspricht dem Template-Namen (Punkte werden zu Schrägstrichen)
- [ ] Alle i18n-Schlüssel in `i18n.properties`, `_de` und `_en`
- [ ] Facet-Schlüssel mit `visible: false` ausgeblendet, falls Standard-Facet ersetzt wird
- [ ] Bilder/Daten nur same-origin über eigene Route laden (siehe 10.10)
- [ ] `cargo test` und `cargo check --workspace` grün
- [ ] Image neu gebaut, Browser hart neu geladen, Konsole geprüft

### 10.9 Fallstricke aus der Praxis

| Symptom | Ursache | Lösung |
|---|---|---|
| Section erscheint nicht, kein Fehler | Template-Name falsch oder Fragment-Route fehlt | Namen und Route abgleichen (10.4 Schritt 3) |
| 404 auf `.../Component-preload.js` oder `.../i18n_*.properties` | Namespace nicht gemappt oder Datei fehlt | Mapping über `ui5_paths`, Routen für i18n anlegen |
| Bild-URL mit `null` (`/thumbnail/null/...`) | `$expand` im Draft liefert kein `video` | Fremdschlüssel direkt lesen (`video_ID`, `${ID}`) |
| `FormatException` bei Datum | Mikrosekunden oder leerer String | Normalisierung im Import (`datetime_offset`) |
| CORS/Private-Network-Fehler im Browser | Seite lädt Bild direkt von fremdem Host (`nuc:9000`) | Proxy same-origin über eigene Route |
| Änderung an Fragment wirkt nicht | Eingebettet per `include_str!`, altes Image läuft | Image neu bauen |
| Alte Oberfläche trotz neuem Image | Browser-Cache | Strg+F5 |
| Schlüssel statt Text in der Oberfläche | i18n-Schlüssel in einer Sprachdatei fehlt | in allen Dateien ergänzen |

### 10.10 Regeln für Bilder und Fremddienste

* Die Oberfläche lädt nur von der eigenen Origin. Externe Dienste über eine eigene Route
  (Proxy) anbinden, die serverseitig abruft.
* Der Proxy holt Daten mit einem HTTP-Client. Im `video-store` ist `reqwest` (rustls) eingebunden.
* Upstream-Adresse über Umgebungsvariable konfigurieren (`THUMBNAIL_BASE_URL`), nicht fest
  im Code.

### 10.12 Authentifizierung (Auth-Proxy)

* Die Runtime prüft keine Anmeldung selbst. Ein vorgeschalteter Reverse Proxy (Caddy mit
  oauth2-proxy) authentifiziert den Benutzer und setzt einen Header mit dem Benutzernamen.
* Der fiori-server liest den Header (`AUTH_USER_HEADER`, Standard `X-Forwarded-User`) und
  legt ihn als `CurrentUser` in die Request-Extensions (`fiori-server/src/auth.rs`).
* In einer Erweiterung: `user: Option<axum::Extension<fiori_server::auth::CurrentUser>>`
  als Handler-Parameter. `user.0 .0` ist der Benutzername.
* `AUTH_REQUIRED=true` lehnt Anfragen ohne Header mit `401` ab. Für Produktion setzen.
* **Pflicht im Proxy**: Der Header muss von eingehenden Anfragen entfernt und neu gesetzt werden,
  sonst kann ein Client die Identität selbst vorgeben. Die Runtime darf außerdem nur über den
  Proxy erreichbar sein (kein direkter Port nach außen).

### 10.11 Grenzen

* Fragmente und Routen sind zur Compile-Zeit eingebettet. Änderungen erfordern einen Rebuild.
* Der Designer-Modus kennt keine Erweiterungen; nur `video-runtime` und eigene Binaries mit
  `run_with`.
* `thumbnail_index 0` ist Konvention für das "erste" Bild. Videos ohne diesen Index zeigen ein
  leeres Bild.
* Schlüssel von Facets und Spalten müssen eindeutig sein. Gleiche Schlüssel überschreiben einander.

---

