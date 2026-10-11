# video-store und fiori-server: Verbesserungsvorschläge

Stand: nach Einführung des Extension-Interfaces, Vorschaubild-Section, Vorschau-Spalte und
Datumsnormalisierung. Die Vorschläge sind nach Priorität sortiert. Jeder Punkt nennt Ursache
bzw. Beleg im Code, den Vorschlag und eine grobe Aufwandsschätzung (S = unter einem Tag,
M = ein bis zwei Tage, L = mehr).

---

## 1. Sicherheit

### 1.1 Proxy ist ohne Authentifizierung erreichbar (Priorität hoch, Aufwand S bis M)

- **Beleg**: `video-store/src/thumbnails.rs`, Route `/ext/video/thumbnail/{video_id}/{n}`.
  Jeder mit Netzzugriff auf Port 3035 kann Bilder vom Thumbnail-Dienst abrufen, auch ohne Login.
- **Vorschlag**: Die Runtime ist aktuell nicht authentifiziert. Vor produktivem Einsatz sollte
  eine Schicht davor stehen (Reverse Proxy mit Basic Auth oder SSO, oder ein Auth-Hook im
  fiori-server). Der Proxy selbst sollte nur Bilder ausliefern, deren Video existiert und nicht
  gelöscht ist (`deleted_at` prüfen).

### 1.2 Upstream-Adresse ist frei konfigurierbar (Priorität mittel, Aufwand S)

- **Beleg**: `THUMBNAIL_BASE_URL` wird ungeprüft verwendet. Ein falsch gesetzter Wert kann den
  Server zu internen Zielen umleiten (SSRF-Risiko).
- **Vorschlag**: Schema und Host gegen eine Allowlist prüfen, oder die Upstream-Adresse nur
  beim Start validieren.

### 1.3 Eingaben im Proxy validieren (Priorität mittel, Aufwand S)

- **Beleg**: `n` ist `u32` und wird ungefiltert in die Upstream-URL gesetzt. Das ist dank Typ
  unkritisch, sollte aber gegen die tatsächliche Anzahl (`thumbnail_count`) geprüft werden, um
  unnötige Upstream-Anfragen zu vermeiden.

---

## 2. Robustheit und Betrieb

### 2.1 Eingebettete Fragmente erzwingen Rebuild (Priorität hoch, Aufwand S)

- **Beleg**: `include_str!` in `thumbnails.rs` für Fragmente und i18n-Dateien.
- **Vorschlag**: Fragmente und Sprachdateien zur Laufzeit aus einem Verzeichnis lesen (z. B.
  `EXTENSION_DIR`), optional mit Fallback auf die eingebettete Version. Dann genügt ein
  Dateiabgleich ohne Image-Build.

### 2.2 Proxy erzeugt pro Bild eine Store-Abfrage (Priorität mittel, Aufwand S)

- **Beleg**: `agent_of()` liest das Video bei jeder Bildanfrage aus dem Store, bei Draft-Fallback
  sogar zweimal.
- **Vorschlag**: Ergebnis (Video-ID → Agent-ID) in einem kleinen Cache halten (z. B. `moka`
  oder eine `HashMap` mit Ablauf). Die Antwort des Proxys trägt bereits `Cache-Control`, damit
  ist der Browser-seitige Effekt gut, serverseitig fehlt aber noch der Cache.

### 2.3 Upstream-Fehler werden auf 404 abgebildet (Priorität mittel, Aufwand S)

- **Beleg**: `thumbnail()` gibt bei jedem Nicht-2xx-Status `404` zurück. Ein Ausfall des
  Thumbnail-Dienstes (500, Timeout) sieht für den Client wie "Bild fehlt" aus.
- **Vorschlag**: Upstream-Status durchreichen bzw. `502` bei Verbindungsfehlern und `504` bei
  Timeouts setzen. Außerdem einen Timeout auf dem `reqwest::Client` setzen (aktuell keiner).

### 2.4 Fehlender Zeitlimit-Schutz und Logging (Priorität mittel, Aufwand S)

- **Vorschlag**: Timeout (z. B. 5 s) für Upstream-Anfragen, `tracing`-Logeinträge für Fehler
  mit Video-ID und Upstream-URL. Der Proxy protokolliert derzeit nichts.

### 2.5 Keine Tests für Proxy und Fragmente (Priorität hoch, Aufwand M)

- **Beleg**: Es gibt Tests für Datumsnormalisierung und Mapping, aber keine für `thumbnails.rs`.
- **Vorschlag**:
  * Unit-Test für die Upstream-URL-Bildung und die Zuordnung Video → Agent (mit gefälschtem Store).
  * Integrationstest mit einem lokalen HTTP-Server als Upstream (z. B. `wiremock` oder ein
    kleiner `axum`-Server in den Tests).
  * Test, dass das Fragment wohlgeformtes XML ist und die Route es ausliefert.

---

## 3. Datenmodell und Fachlichkeit

### 3.1 "Erstes Bild" als Konvention (Priorität mittel, Aufwand S bis M)

- **Beleg**: Vorschau-Spalte und Proxy-Aufruf nutzen `thumbnail_index 0`. Die Konvention ist
  im Code nicht abgesichert.
- **Vorschlag**: Im Store ein berechnetes Feld `first_thumbnail_index` bzw. eine Referenz auf
  das erste Thumbnail im Migrationsschritt anlegen. Alternativ im Proxy ein Fallback auf das
  kleinste vorhandene `thumbnail_index` einbauen.

### 3.2 `thumbnail_count` und tatsächliche Records können auseinanderlaufen (Priorität mittel, Aufwand S)

- **Beleg**: Im Test zeigte `thumbnail_count = 13` für ein Video die gültigen Indizes 0 bis 12.
  Ob jeder Record auch als Datei beim Thumbnail-Dienst existiert, ist nicht geprüft.
- **Vorschlag**: Einmaliger Abgleich (Skript oder Testlauf): Anzahl der Records je Video gegen
  `thumbnail_count` und gegen den Thumbnail-Dienst prüfen. Abweichungen in einem Bericht
  auflisten.

### 3.3 Legacy-IDs sind nicht sichtbar (Priorität mittel, Aufwand S)

- **Beleg**: Video und Agent haben laut `ddic.json` kein `legacy_id`-Feld. Die Bild-URL
  verwendet die OData-ID. Für Support und Abgleich mit dem Thumbnail-Dienst fehlt die
  Original-ID in der Oberfläche.
- **Vorschlag**: Falls die Original-ID im Store existiert, als Feld im Modell ausweisen (Neugenerierung
  der `ddic.json`), z. B. im Object Page Header.

### 3.4 Datumsnormalisierung nur im video-store (Priorität niedrig, Aufwand M)

- **Beleg**: `datetime_offset` in `video-store/src/import.rs`.
- **Vorschlag**: Generisch im Store bzw. Import-Schicht des fiori-server anbieten, sobald ein
  zweiter Anwendungsfall entsteht. Dann muss nicht jede Erweiterung das Problem neu lösen.

### 3.5 Zeitstempel-Typ als Feldattribut (Priorität niedrig, Aufwand M)

- **Vorschlag**: Mikrosekunden bleiben im Store erhalten, die Anzeige kürzt. Heute gehen sie
  beim Import verloren. Für Audit-Zwecke wäre das relevant.

---

## 4. Fiori-Elements-Oberfläche

### 4.1 Auflösung der `{{…}}`-Schlüssel nicht überall verifiziert (Priorität hoch, Aufwand S)

- **Beleg**: Die Spaltenüberschrift `{{previewColumnTitle}}` wurde noch nicht im Browser
  bestätigt.
- **Vorschlag**: Im Browser prüfen. Falls nicht aufgelöst, Text fest setzen oder den Schlüssel
  über das Manifest `i18n`-Modell auflösen.

### 4.2 Launchpad-Intent und App-Namespace sind unterschiedlich (Priorität niedrig, Aufwand M)

- **Beleg**: Intent `Video-display`, App-ID `videos`, Pfad `/apps/Video/`. Der Pfad ist über die
  Hauptentität abgeleitet, die App-ID über den App-Namen.
- **Vorschlag**: Eine einheitliche Konvention festlegen. Die Mapping-Logik in `flp.rs` ist
  bereits mehrfach verzweigt (Lookup über Hauptentität, Namespace über App-Name). Eine
  zentrale Funktion `app_path(app)` würde Fehler vermeiden.

### 4.3 Preload-Antwort ist ein Notbehelf (Priorität niedrig, Aufwand S)

- **Beleg**: `component_preload_handler` liefert eine leere Datei, damit UI5 nicht fehlschlägt.
- **Vorschlag**: Eigentliche Ursache beheben (korrekter Komponentenpfad), dann den Handler entfernen.

### 4.4 Konsolenwarnungen aufräumen (Priorität niedrig, Aufwand M)

- `Command 'DeleteEntry::ContextMenu' is not defined in component manifest`: Aktion im Manifest
  definieren oder die Kontextmenü-Standardaktion gezielt abschalten.
- `PropertyInfo validation is disabled`: auf die `propertyInfo`-Konfiguration migrieren, sobald
  die Tabellen stabil sind.
- `Permissions policy violation: unload`: stammt aus dem UI5-Framework; kein Handlungsbedarf,
  aber dokumentieren.
- `Cannot add user action` / `control id not allowed for header items`: Shell-Meta, die
  Launchpad-Konfiguration (`site.json`) kann bereinigt werden.

### 4.5 Flex-Connector antwortet mit 404 (Priorität niedrig, Aufwand S)

- **Beleg**: `/sap/bc/lrep/flex/data/videos` fehlt. Harmlos, erzeugt aber Log-Rauschen beim Start.
- **Vorschlag**: Entweder Flex deaktivieren (`flexEnabled: false` im Manifest) oder eine leere
  Antwort liefern.

### 4.6 Detailliertes Design der Thumbnail-Section (Priorität niedrig, Aufwand M)

- Bildgröße fest auf 200 px. Responsive Spaltenbreite wäre sinnvoll.
- Klick auf ein Bild öffnet nichts. Vorschlag: Vollbild-Dialog mit Navigation vor/zurück.
- Ladeanzeige fehlt. Bei vielen Bildern entstehen sichtbare Lücken.

---

## 5. Erweiterungs-Interface (fiori-server)

### 5.1 Trait ohne Versionierung und ohne Registrierung per Konfiguration (Priorität mittel, Aufwand M)

- **Beleg**: Erweiterungen werden in `video-runtime.rs` fest verdrahtet.
- **Vorschlag**: Konfigurierbare Auswahl (z. B. über Feature-Flags oder eine Konfigurationsdatei)
  und eine Versionsnummer des Traits, damit externe Crates kompatibel bleiben.

### 5.2 Konflikte zwischen Erweiterungen werden nicht erkannt (Priorität mittel, Aufwand S)

- **Beleg**: `object_page_content` und `list_columns` überschreiben gleiche Schlüssel still.
- **Vorschlag**: Beim Zusammenführen Konflikte loggen (`tracing::warn`), damit doppelte
  Schlüssel auffallen.

### 5.3 Erweiterungen können nicht im Designer-Modus laufen (Priorität niedrig, Aufwand M)

- Der Designer kennt das Trait nicht. Für Entwicklung eigener Erweiterungen wäre ein
  Designer-Modus mit denselben Erweiterungen nützlich.

### 5.4 Hook-Modell nicht mit Erweiterungen verbunden (Priorität niedrig, Aufwand M)

- **Beleg**: `hook.rs` (Vor- und Nachverarbeitung von Operationen) und das neue Trait sind
  getrennte Mechanismen.
- **Vorschlag**: Ein Trait mit optionalen Hook-Methoden anbieten, damit eine Erweiterung
  Datenlogik und UI in einem Paket liefern kann.

---

## 6. Repository und Pflege

### 6.1 Uncommittete Änderungen aus früheren Arbeiten (Priorität hoch, Aufwand S)

- **Beleg**: `git status` zeigt viele geänderte Dateien in `fiori-server`, `spec`,
  `sqlite-relational-store` und `video-store`, die nicht aus den hier beschriebenen Änderungen
  stammen. Die neuen Dateien (`extension.rs`, `thumbnails.rs`, `video-runtime.rs`, Fragmente,
  i18n, Dokumente) sind ebenfalls noch nicht committet.
- **Vorschlag**: Aufteilen in thematische Commits (Extension-Interface, video-store-Erweiterung,
  Datumsnormalisierung, Dokumentation). Vorher prüfen, welche fremden Änderungen gewollt sind.

### 6.2 Dokumentation als Single Source of Truth (Priorität mittel, Aufwand S)

- **Beleg**: `fiori-elements-de.md` ist groß und enthält sowohl Ist-Zustand als auch Anleitung.
  Einige Abschnitte (8.x) beschreiben noch die Lage vor dem Extension-Interface.
- **Vorschlag**: Ist-Zustand, Anleitung und Verbesserungen trennen (diese Datei für Letzteres).
  Abschnitt 8 kürzen und auf Abschnitt 10 verweisen.

### 6.3 Docker-Build lädt alles bei jeder Änderung (Priorität niedrig, Aufwand S)

- **Beleg**: `COPY . .` im Dockerfile des video-store. Jede Dokumentationsänderung invalidiert
  den Build-Cache.
- **Vorschlag**: `.dockerignore` anlegen (ohne `target/`, `docs/`, `*.md`), damit nur relevante
  Änderungen den Build auslösen.

### 6.4 `Cargo.lock`-Änderungen durch neue Abhängigkeiten (Priorität niedrig, Aufwand S)

- `reqwest` (rustls) und `axum` wurden neu in `video-store` aufgenommen. Die Lock-Datei sollte
  bewusst committet und die Auswahl der TLS-Implementierung dokumentiert werden.

---

## 7. Vorschlag für die nächsten Schritte

1. Sicherheit (1.1): Auth-Schicht vor der Runtime klären.
2. Tests für Proxy und Fragmente (2.5) und Verifikation der i18n-Auflösung (4.1).
3. Fragmente zur Laufzeit laden (2.1), damit Oberflächenanpassungen ohne Rebuild gehen.
4. Upstream-Fehler differenzieren und Timeout setzen (2.3, 2.4).
5. Commit-Aufteilung (6.1), damit der Stand nachvollziehbar ist.
6. Erstes Bild im Store verankern (3.1).

Die übrigen Punkte sind Qualitätsverbesserungen, die bei Bedarf eingeplant werden können.
