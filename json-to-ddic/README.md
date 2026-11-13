# json-to-ddic

Separates CLI zur Ableitung eines initialen PJL-DDIC aus fachlichem Beispiel-JSON
oder -XML.
Keine Serververbindung erforderlich. Die Ausgabe wird mit `spec` und
`spec-schema` validiert.

```powershell
cargo run -p json-to-ddic -- .\orders.json --entity Order --package sales --out .\sales-ddic.json
Get-Content -Raw .\orders.json | cargo run -p json-to-ddic -- - --entity Order --out .\sales-ddic.json
cargo run -p json-to-ddic -- .\orders.xml --entity Orders --package sales --out .\sales-ddic.json
Get-Content -Raw .\orders.xml | cargo run -p json-to-ddic -- - --format xml --entity Orders --out .\sales-ddic.json
```

Bestehende Dateien werden nur mit `--force` ueberschrieben.
Warnungen gehen nach stderr. Es werden weder Beispieldaten noch Apps erzeugt.

## XML-Eingabe

`--format auto` (Standard) erkennt XML an der Dateiendung `.xml`
(unabhaengig von Gross-/Kleinschreibung); alle anderen Dateinamen und stdin
werden weiterhin als JSON gelesen. `--format xml` bzw. `--format json`
ueberschreibt die Auswahl. Eingaben muessen UTF-8 sein, ein BOM ist erlaubt.

XML wird zuerst in eine JSON-artige Struktur ueberfuehrt und dann nach denselben
DDIC-Regeln abgeleitet:

- Der Inhalt des Wurzelelements bildet die Wurzelentitaet. Deren Name wird
  weiterhin mit `--entity` festgelegt; der XML-Wurzelname wird nicht als
  zusaetzliche Ebene uebernommen.
- Textwerte und Attribute bleiben immer Strings, auch `001`, `true` oder `12.5`.
- Attribute erhalten `@name`, Text in Elementen mit Attributen oder Kindern
  erhaelt `#text`. Die normale DDIC-Namensnormalisierung macht daraus z. B.
  `_name` und `_text` und meldet diese Umbenennung.
  Sind Attribute oder Kinder an einem Elementpfad nur teilweise vorhanden,
  bleibt die Objektstruktur auch fuer die anderen Vorkommen erhalten.
- Gleichnamige wiederholte Kindelemente werden Listen. Wird an einem Elementpfad
  irgendwo eine Wiederholung beobachtet, werden auch einzelne Vorkommen an
  diesem Pfad als Liste behandelt. Ohne beobachtete Wiederholung wird keine
  Liste geraten.
- Leere Blattelemente werden leere Strings. Whitespace-only-Text zwischen
  Kindelementen wird ignoriert, sonstiger Text zusammengefuehrt.
  Zeilenenden und Attribut-Whitespace werden nach XML-1.0-Regeln normalisiert.
  CDATA und vordefinierte/numerische XML-Entities werden dekodiert.
- Qualifizierte Namen behalten ihren Namespace-Praefix; `xmlns`-Deklarationen
  sind keine fachlichen Felder. Namespace-URIs werden nicht kanonisiert.
- Kommentare und Processing Instructions werden ignoriert. XML 1.0 wird
  unterstuetzt; DTDs, benutzerdefinierte Entities und andere deklarierte
  Encodings werden mit Fehler abgewiesen.

Das ist kein XML-Roundtrip: Reihenfolge unterschiedlich benannter Kinder und
Textpositionen in Mixed Content gehen verloren. `xsi:nil` wird nicht speziell
interpretiert. Fachliche XSD-Typen und Kardinalitaeten werden nicht ausgewertet.

## Ableitungsregeln

- Ein Wurzelobjekt wird eine Entitaet; eine Wurzelliste liefert deren Datensaetze.
  Skalare Wurzeln erhalten ein `value`-Feld.
- Alle Listenelemente werden ausgewertet. Fehlende Felder und `null` liefern keine
  Typinformation; bekannte Typen anderer Datensaetze werden weiterverwendet.
- Texte bleiben `Edm.String` (keine spekulative Datums-, UUID- oder Code-Erkennung).
  Boolean wird `Edm.Boolean`, Ganzzahlen werden `Edm.Int32` bzw. `Edm.Int64`,
  sonstige Zahlen `Edm.Decimal`. Numerische Mischungen werden erweitert.
  Stringlaengen werden nicht aus Stichproben begrenzt. Dezimalfelder erhalten
  mindestens Precision 38 und Scale 3; beobachtete groessere Werte bzw.
  Nachkommastellen erweitern diese Grenzen (auch bei Exponentialschreibweise).
  Diese Grenzen sind fuer zukuenftige Daten fachlich zu pruefen.
- Unterobjekte werden eigene Entitaeten mit einwertiger Referenz im Elternobjekt.
  Die DDIC-Beziehung ist technisch N:1, nicht eine erzwungene 1:1-Komposition.
- Listen werden Kind-Entitaeten mit Kompositionsbeziehung; primitive Elemente
  erhalten ein `value`-Feld. Verschachtelte Listen werden rekursiv abgebildet.
  `_index` speichert bei einem spaeteren Import die nullbasierte Listenposition.
- Nur `null`, leere Listen und widerspruechliche Typen werden mit Warnung als
  `Edm.String` angelegt. Beim spaeteren Import muessen solche Werte als JSON-Text
  serialisiert werden. Das gilt bei heterogenen Listen fuer die gesamte Liste.
  Bei heterogenen Wurzellisten wird dagegen jedes Element als `value`-JSON-Text
  eines Wurzeldatensatzes gespeichert.
- Feldnamen werden zu ASCII-Bezeichnern normalisiert, Kollisionen nummeriert.
  Labels behalten den urspruenglichen Namen; Warnungen nennen JSON-Pfad und
  abweichenden Feld-/Navigationsnamen. Entitaetsnamen folgen dem Objektpfad.
- Technische UUID-IDs werden vom DDIC bereitgestellt. Ein fachliches `ID` wird
  umbenannt; fachliche Schluessel werden nicht geraten. Drafts und Auditierung
  sind im initialen Modell deaktiviert.

Dies ist ein **Modellentwurf, kein Datenimport und kein verlustfreies JSON-Schema**.
Ein spaeterer Import muss Referenzen, IDs und Listenpositionen setzen.
`null` gegen fehlende Werte, leere gegen fehlende Listen und `null`-Elemente in
Objektlisten sind relational nicht unveraendert unterscheidbar. Nicht beobachtete
Strukturen, Pflichtfelder, Eindeutigkeit, Waehrungen und fachliche Beziehungen
muessen anschliessend ergaenzt werden.
Die Zahleninterpretation folgt `serde_json` (Dezimalzahlen als IEEE-754-f64);
beliebig genaue Dezimalliterale sind daher nicht verlustfrei rekonstruierbar.

```powershell
cargo test -p json-to-ddic
```
