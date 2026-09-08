# Navigation-Abhängigkeit-Lücke: Gap-Analyse & Erweiterungspunkte

> Verifikation der Vermutung „`YamlStore` unterscheidet aktuell nicht zwischen
> abhängiger Navigation und freier Assoziation."

---

## 1. Beobachtung

Der Prototyp führt Navigations-Beziehungen **einheitlich** als `Query-time-`
Struktur (`Navigation`) ein und löst sie im OData-Endpoint über `$expand`
auf. Es gibt keine persistierende Metadaten-Definition der Beziehung und keine
Propagierung der Draft-Transaktion auf abgehängige Kind-Objekte.

---

## 2. Beleg: `Navigation` = rein Query-time-Context

Struktur in `datastore.rs`:

```rust
pub struct Navigation {
    pub single: bool,
    pub from_set: Option<String>,
    pub from_field: Option<String>,
    pub to_set: Option<String>,
    pub to_field: Option<String>,
}
```

Das sind **nur Dimension-Namen** (which-Entität/Zu-Feld-welche-Entität-Zu-Feld),
nicht `foreignKey`/`NavigationConfiguration`/`dependency`. `Navigation` hat nur
`from()`, `to()` und `new(single)`.

Nutzung (nur im Query-Pfad, nie bei Activate/Edit/Dataschema-Schema):

```rust
fn navigation(&mut self, single: bool) -> &mut Navigation {
    let navigations = self.navigations.get_mut().expect("navigations lock");
    navigations.push(Navigation::new(single));
    navigations.last_mut().expect("last navigation")
}
```

`self.navigations` wird nur pro Query via `set_navigations()`/`navigation()`
befüllt; beim Laden/Ändern eines Datensatzes fließt das Data-Dictionary nicht
in die Persistenz-/Draft-Logik ein. Es existiert kein Analog von
`NavigationConfiguration`, aus dem hervorgeht, ob die Beziehung **abhängige**
bzw. **free-form** ist.

---

## 3. Beleg: Resolve-Time ist reines free-form `$expand`

`expand_single_row` (datastore.rs ~L128) resolt beide Beziehungen als pure ID-Abgleich:

```rust
Ok(if *single {
    if let Some(from_id) = row.get(from_id_field.as_str()) {
        let q = ODataQuery::new().filter(Expr::equals_value(from_id, "ID"));
        let subrows = self.get_collection(to_entity, &q)?.map(|x| x.core_json()).collect();
        match subrows.as_slice() {
            []      => row[from_field] = Value::Null,
            [v]     => row[from_field] = json!(v),
            _       => todo!(),   // mehrere children -> fallback
        }
    } else { row[from_field] = Value::Null }
} else {
    let q = ODataQuery::new().filter(Expr::equals_guid(row.id(), to_field));
    let subrows = self.get_collection(to_entity, &q)?.map(|x| x.core_json()).collect();
    row[from_field] = subrows;
})
```

→ **Keine FK-Adressierung**, nur `equals_value/equals_guid`. `single` = der einzige
Unterschied (Kollektion vs. Single-Navigation), keine FK-Definition. Exakt die freie
Assoziation.

---

## 4. Beleg: Edit / Activate propagieren keine Child-Drafts

- `DataRecord::edit()` und `create_draft()` ändern **nur** den eigenen Datensatz
  (active-Flag + DraftAdmindaten). Kein Aufruf an `read_sibling_entity` bzw.
  Kind-Objekt.
- `DataRecord::activate()` setzt `is_active_entity = true`, `has_draft_entity = false`.
- `draft_activate()` (yaml_store.rs):

  ```rust
  draft.activate()?;
  self.write_record(set_name, draft.clone())?;
  self.remove_record(set_name, key)?;   // ← nur der eigene Satz
  ```

- `read_sibling_entity()` in `YamlStore` ist `todo!()` – die Abfrage des
  Sibling/Child-Drafts ist gar nicht implementiert.

Es fehlt also die Kern-Aktion: „Bei `edit()` eines Elternsatzes, der über eine
**abhängige Navigation** verfügt, wird für `to_field` eine neue Child-Draft-Instanz
generiert (IsActiveEntity = false). Bei `activate()` wird die vorhandene
`read_sibling_entity`-Draft-Wiederaufnahme ausgeführt." Das ist genau die Logik,
die SAP-BDxf/rapid pro Navigation-Typ erwartet. Beide Fälle führen hier blind über `$expand`–free-form-Resolve.

---

## 5. Lücken-Matrix (erwartet vs. implementiert)

| Konzept | Erwartet (Rap/BDxf) | Implementiert im Snapshot |
|---|---|---|
| Metadaten-FK-Definition | `NavigationConfiguration`/`foreignKey` (Set + Zu-Feld + Kind-Feld + FK-Name) | **fehlend** |
| Abhängige Navigation → Draft-Wiederaufnahme | `edit()` generiert Child-Draft automatisch | **keine Propagierung** / kein `read_sibling_entity` |
| Abhängige Navigation → Draft-Abwertung | `activate()` aktiviert Child-Existent weiter | **keine Propagierung** |
| Free-form → eigenständiger Draft | Kind-Objekt eigenständig in Draft/Active | frei-form-Expand, Draft wird nur beim direkten Edit-/Activate des Kindes erzeugt |
| Distinguish-Type-Flag | „abhängig" vs „free" als Eigenschaft der Navigation | **einheitlich** (keine Unterscheidung) |

---

## 6. Konkrete Erweiterungspunkte (keine Änderungen durchgeführst)

Wenn eine korrekte Differenz ergänzt wird, am nötigensten:

1. **`Navigation` um FK-Metadaten erweitern**
   Hinzugabe `to_field`-Adressierung (`<KindField>_ID`), `fk`-Pfad (set), und
   eine Option `dependent: bool` (oder `dependency_type`). Erst wenn die Navigation
   die Beziehung-Definition, die „abhängig vs. frei" kennzeichnet.
2. **`edit()` → Child-Draft erzeugen**
   Für jede `Navigation` mit `dependent = true`, die auf den aktiv geänderten Satz
   zutrifft, Child-Draft-Instanz anlegen (IsActiveEntity = false), ansonsten freier
   Form lässt den eigenen Satz.
3. **`activate()` → Child-Transaktions-Wiederaufnahme**
   Für `dependent`-Navigations: `read_sibling_entity()` findet und aktiviert den
   bereits bestehenden Child-Draft.
4. **`read_sibling_entity()` implementieren**
   Bisher `todo!()` – zentral für die Wiederaufnahme (Punkt 3).
5. **`NavigationConfiguration`-Metadaten-Schema**
    persistence, aus dem beim Reload „abhängige vs. free" hervorgeht.

---

## 7. Fazit

Die Vermutung ist **verifiziert**:
- Navigation ist **Query-only** (`Navigation`-Struktur gibt), keine Metadaten-Referenz.
- Resolve-Time ist **rein free-form** (`equals_value`/`equals_guid`, kein FK).
- Draft/Edit/Activate **propagieren nicht** – keine Unterscheidung „abhängige vs. frei",
  nur Query-time `$expand` + free-form.

Der Prototyp deckt die `free-form`-Semantik korrekt ab; die **abhängige-
Navigation** (Draft-Transaktions-Kopplung in Kind-Objekte) wird derzeit **nicht
behandelt** – eine neue Logik/Definition (z. B. `Navigation::dependent`,
`read_sibling_entity`) wäre nötig, um diese Lücke geschlossen.
