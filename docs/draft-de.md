# CAP Draft Handling — formale Spezifikation

Diese Spezifikation destilliert das CAP-Draft-Verhalten (Node.js- und Java-Runtime,
Stand CAP 2026) auf die Zustände, Übergänge und Invarianten, die für eine eigene
`DataStore`-Implementierung relevant sind. Quellen: capire Cookbook "Serving SAP
Fiori UIs", Node.js "Fiori Support", Java "Fiori Drafts".

## 1. Datenmodell

Pro draft-fähiger Entität `E` (annotiert mit `@odata.draft.enabled`) existieren
**zwei physische Tabellen**, aber **ein logisches OData-Entity-Set**:

- **Active-Table**: normale Datenhaltung, Primärschlüssel wie in `E` definiert.
- **Draft-Table** (`E.drafts`): strukturell identisch zu `E` (inkl. aller
  Kompositions-Kinder — Drafts vererben sich rekursiv über `Composition`,
  nicht über `Association`), plus technische Zusatzfelder.

Jede Instanz — egal ob aus Active- oder Draft-Table gelesen — bekommt zur
Laufzeit einen erweiterten Schlüssel und berechnete Felder:

| Feld | Typ | Bedeutung |
|---|---|---|
| `IsActiveEntity` | Boolean, Teil des Composite Key | `true` = Zeile aus Active-Table, `false` = Zeile aus Draft-Table |
| `HasActiveEntity` | Boolean, computed | Existiert zu diesem Draft ein Active-Pendant? (Edit-Draft vs. New-Draft) |
| `HasDraftEntity` | Boolean, computed | Existiert zu dieser Active-Zeile ein offener Draft? |
| `DraftAdministrativeData` | Assoziation | `DraftUUID`, `CreatedByUser`, `LastChangedByUser`, `InProcessByUser`, `CreationDateTime`, `LastChangeDateTime`, optional Validierungsnachrichten |

**Invariante I1**: Zu jedem Zeitpunkt existiert für ein gegebenes Business-Key-Tupel
höchstens eine Active-Zeile und höchstens eine Draft-Zeile. `HasDraftEntity` auf der
Active-Zeile und `HasActiveEntity` auf der Draft-Zeile sind zueinander konsistent.

**Invariante I2 (New-Draft vs. Edit-Draft)**:
- *New-Draft*: `IsActiveEntity=false`, `HasActiveEntity=false` — es gibt keine
  korrespondierende Active-Zeile, der Draft ist die einzige Repräsentation.
- *Edit-Draft*: `IsActiveEntity=false`, `HasActiveEntity=true` — Active-Zeile
  existiert unverändert parallel weiter, bis der Draft aktiviert oder verworfen wird.

## 2. Zustandsautomat (pro Business-Key)

```
                 NEW ──────────────► [New-Draft]
                                         │
                                         │ SAVE (activate)      CANCEL
                                         ▼                         │
   ┌──────────► [Active, kein Draft] ◄───┴─────────────────────────┘
   │                    │
   │ EDIT               │ DELETE (aktiv)
   │                    ▼
   │              [gelöscht]
   │                    ▲
   └── [Active+Draft] ──┘
              │      DELETE (aktiv, mit Draft) löscht Active + Draft
              │
              ├── PATCH (mehrfach, idempotent bzgl. Zustand) ──► [Active+Draft]
              ├── CANCEL ──► [Active, kein Draft]  (Active unverändert)
              └── SAVE (activate) ──► [Active, kein Draft]  (Active = Draft-Inhalt)
```

Es gibt genau **zwei Wege in den Draft-Zustand** (NEW, EDIT) und genau
**zwei Wege wieder heraus** (CANCEL, SAVE). Kein anderer Übergang ist zulässig.

## 3. Aktionen im Detail

| # | Aktion | Trigger (HTTP) | Precondition | Effekt | intern ausgelöste Events |
|---|---|---|---|---|---|
| 1 | **NEW** | `POST /E` (leerer Body, oder Custom Action via `@Common.DraftRoot.NewAction`) | — | Legt neue Zeile in Draft-Table an. `IsActiveEntity=false`, `HasActiveEntity=false`. Schlüssel (z. B. UUID) wird vom Server vergeben. | `DRAFT_NEW` → intern `DRAFT_CREATE` |
| 2 | **EDIT** | `POST /E(key,IsActiveEntity=true)/draftEdit` | Active-Zeile muss existieren; darf nicht bereits durch anderen Draft gesperrt sein (siehe §4) | Kopiert Active-Zeile (rekursiv über gesamte Komposition) in Draft-Table. `IsActiveEntity=false`, `HasActiveEntity=true`. Active-Zeile bleibt unverändert. | `DRAFT_EDIT` → intern `DRAFT_CREATE` |
| 3 | **PATCH** | `PATCH /E(key,IsActiveEntity=false)` | Draft-Zeile muss existieren und darf nicht durch anderen User gesperrt sein | Feldweise Änderung der Draft-Zeile. Kein Effekt auf Active-Zeile. Läuft beliebig oft. Nach jedem PATCH i. d. R. Nachladen von Validierungsnachrichten (State Messages). | `DRAFT_PATCH` |
| 4 | **CANCEL / DISCARD** | `DELETE /E(key,IsActiveEntity=false)` | Draft-Zeile muss existieren | Löscht die Draft-Zeile (rekursiv über Komposition). Active-Zeile bleibt **exakt** im Zustand vor `EDIT` — keine partiellen Übernahmen. | `DRAFT_CANCEL` |
| 5 | **SAVE / ACTIVATE** | `POST /E(key,IsActiveEntity=false)/draftActivate` | Draft-Zeile muss existieren, alle `@mandatory`/`@assert`-Validierungen müssen bestehen | Siehe §5 (Aktivierung) | `DRAFT_SAVE` → `CREATE` (New-Draft) oder `UPDATE` (Edit-Draft) |
| 6 | **DELETE aktiv** | `DELETE /E(key,IsActiveEntity=true)` | Active-Zeile muss existieren | Löscht Active-Zeile. Falls ein Draft existiert, wird dieser ebenfalls gelöscht (kein verwaister Draft). | `DELETE` (+ `DRAFT_CANCEL`, falls Draft vorhanden) |
| 7 | **READ** | `GET /E` bzw. `GET /E(key,IsActiveEntity=…)` | — | Ohne Filter: Merge aus Active- und Draft-Tabelle, pro Business-Key genau eine sichtbare Zeile (Draft hat Vorrang, falls vorhanden — UI zeigt "in Bearbeitung"). Mit explizitem `IsActiveEntity`-Filter: gezielter Zugriff auf genau eine der beiden Tabellen. Sonderfall Fiori-Filter "Editing Status: All": zwei separate Reads (Active, Draft), Ergebnisse client- bzw. serverseitig kombiniert, **nicht** gemergt. | `READ` → intern `ACTIVE_READ` + `DRAFT_READ` |

**Wichtig für die eigene Implementierung**: Aktionen 1–6 sind die einzigen
zulässigen Zustandsübergänge. Freies Filtern/Schreiben über `IsActiveEntity`
außerhalb dieser sechs Pfade ist laut Spezifikation nicht vorgesehen
("Manual filtering on draft-related properties is not allowed").

## 4. Sperrverhalten (Draft Lock)

- Sobald ein Draft existiert (Zustand `Active+Draft` oder `New-Draft`), ist die
  zugehörige Entität für **andere** User gegen `EDIT` gesperrt (nicht gegen `READ`).
- Der Lock wird über `DraftAdministrativeData.InProcessByUser` gehalten.
- Auflösung des Locks durch genau drei Ereignisse:
  1. `SAVE` (Draft aktiviert)
  2. `CANCEL` (Draft verworfen)
  3. **Timeout** — Default 15 Minuten, konfigurierbar
     (`cds.drafts.cancellationTimeout` / vergleichbare Node-Property). Nach
     Timeout wird der Lock serverseitig freigegeben (nicht zwingend die
     Draft-Zeile selbst gelöscht).
- Lock-Prüfung ist optional abschaltbar (`cds.security.draftProtection.enabled=false`),
  dann sind parallele `EDIT`-Aufrufe verschiedener User auf dieselbe Entität möglich
  — Konfliktbehandlung liegt dann außerhalb der Draft-Semantik.

**Für den eigenen Store**: Lock ist ein separates, orthogonales Feature zur
State-Machine — im Kern ein `(key) -> (owner, expiry)`-Mapping, das bei EDIT
gesetzt und bei SAVE/CANCEL/Timeout gelöscht wird.

## 5. Aktivierung (SAVE) — Transaktionale Garantien

Dies ist der komplexeste und kritischste Übergang. Reihenfolge **innerhalb
einer Transaktion**:

1. Validierung der Draft-Daten (inkl. rekursiv aller Komposition-Kinder).
2. Draft-Daten werden aus der Draft-Table **gelöscht**.
3. Danach wird — mit denselben Daten — auf der Active-Table ein
   `CREATE` (falls New-Draft, `HasActiveEntity=false`) oder `UPDATE`
   (falls Edit-Draft, `HasActiveEntity=true`) ausgeführt — rekursiv über die
   gesamte Komposition (Deep Insert/Update: Kinder werden angelegt, geändert
   oder gelöscht, je nachdem was im Draft-Teilbaum vorhanden ist).
4. Lock wird freigegeben.

**Invariante I3 (Atomarität)**: Schlägt Schritt 3 fehl (z. B. DB-Constraint-
Verletzung, Custom-Handler wirft Fehler), wird die **gesamte Transaktion**
zurückgerollt — inklusive der Löschung aus Schritt 2. Der Draft existiert
danach unverändert weiter, als wäre `SAVE` nie aufgerufen worden. Es darf
**niemals** ein Zwischenzustand sichtbar werden, in dem weder Draft noch
konsistente Active-Daten existieren.

**Invariante I4 (Deep Activation)**: Bei Kompositionsbäumen wird der gesamte
Baum atomar aktiviert — kein Teil-Commit einzelner Kind-Entitäten.

## 6. Bypass-Modus (Direktzugriff ohne Draft)

Aktivierbar per Flag (`cds.fiori.bypass_draft` / analog Java). Erlaubt direktes
CRUD auf die Active-Table unter Umgehung der State-Machine:

| HTTP | Effekt |
|---|---|
| `POST /E {..., IsActiveEntity:true}` | Legt Active-Zeile direkt an, kein Draft beteiligt |
| `PATCH /E(key,IsActiveEntity=true)` | Sparse Update direkt auf Active-Zeile |
| `PUT /E(key,IsActiveEntity=true)` | Full Update/Create direkt auf Active-Zeile |

**Wichtig**: Existiert für den betroffenen Key parallel ein Draft, greift diese
Umgehung **nicht automatisch** in den Draft ein — der Draft bleibt bestehen und
überschreibt bei späterem `SAVE` die per Bypass gemachten Änderungen wieder.
Bypass und Draft-Flow dürfen produktiv nicht unkoordiniert gemischt werden.

## 7. Garbage Collection

- Verwaiste bzw. inaktive Drafts (nicht bearbeitet, Lock abgelaufen) werden
  nach einem Timeout automatisch gelöscht — Default 30 Tage
  (`cds.drafts.deletionTimeout` bzw. `cds.fiori.draft_deletion_timeout`).
- Dies ist reines Housekeeping, kein State-Machine-Übergang im engeren Sinn —
  entspricht technisch einem serverseitig ausgelösten `CANCEL`.

## 8. Zusammenfassung als Interface für den eigenen Store

Für eine eigene Implementierung ergibt sich folgende Mindest-API, die alle
sechs Übergänge plus Read/Lock/GC abdeckt:

```
new_draft(entity, initial_data)            -> draft_id            // §3.1
edit_draft(entity, active_key)             -> draft_id            // §3.2, prüft Lock
patch_draft(entity, draft_key, delta)      -> void                // §3.3, prüft Lock
cancel_draft(entity, draft_key)            -> void                // §3.4, löst Lock
activate_draft(entity, draft_key)          -> active_key          // §3.5+§5, transaktional!
delete_active(entity, active_key)          -> void                // §3.6, kaskadiert auf Draft
read(entity, key?, is_active_entity?)      -> row | merged_rows   // §3.7

acquire_lock(entity, key, user) / release_lock(...) / lock_expired(...)  // §4
gc_stale_drafts(older_than)                // §7
```

Die kritischste Design-Entscheidung für die eigene Umsetzung ist §5
(Atomarität von "Draft löschen + Active schreiben" in einer Transaktion mit
Rollback-Garantie) — das ist der Punkt, an dem naive Implementierungen am
ehesten inkonsistente Zwischenzustände erzeugen.
