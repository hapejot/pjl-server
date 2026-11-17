# Prototyp Vorfertigung

Eigenständiges DDIC-Beispiel mit fiktiven Daten: Auftragsliste mit Filterleiste,
Auftragsdetail mit Komponenten, Rückmeldungen, Vorgängen und Dokumenten sowie
einer Detailansicht für Rückmeldungen.

## Start

Im Repository-Wurzelverzeichnis:

```sh
cargo run --release --bin pjl-runtime -- \
  --bind 127.0.0.1 --port 8092 \
  --ddic vorfertigung/ddic.json --data-dir vorfertigung/data \
  --scripts-dir vorfertigung/scripts
```

Das Launchpad ist unter <http://localhost:8092/> erreichbar. Die UI5-Ressourcen
werden über das Netzwerk geladen. Das Datenverzeichnis ist unabhängig von anderen
Serverinstanzen. Der Server liest das Dictionary beim Start; nach Änderungen neu
starten. Die SQLite-Daten werden beim regulären Beenden gespeichert.

Die Beispieldaten werden nur eingespielt, wenn das jeweilige Entity-Set leer ist.
Zum erneuten Einspielen eine neue, leere `--data-dir` verwenden.

## Umfang und Grenzen

Die Darstellung nutzt die vorhandenen Fiori-Elements-Listen, Filter, Kopfdaten und
Facetten. Sie ist an die Screenshots angelehnt, aber keine exakte Nachbildung:
mehrzeilige und farbige Tabellenzellen, spezielle Dialoge und eine gemeinsame
Rückmeldemaske mit Positionstabelle sind nicht allein durch dieses DDIC definiert.
Rückmeldungen sind eigenständige Datensätze mit Mengen und Zeitangaben; Änderungen
an ihnen aktualisieren die Auftragsmengen nicht automatisch.

„Vorgang starten“ und „Vorgang zurücksetzen“ simulieren ausschließlich einen
Statuswechsel am Auftrag. Entnahmebuchung und Rückmeldung mit automatischer
Mengenfortschreibung sind nicht implementiert. Es gibt keine SAP-Anbindung.
Dokumente sind nur Metadaten; die Beispieldatei ist nicht vorhanden. Das Modell
ist ein Demonstrator, nicht für den produktiven Fertigungsbetrieb gedacht.
