#!/usr/bin/env python3
"""Spielt einen mit odata-export.py erzeugten Datenbestand auf einen OData-Service zurueck.

Ablauf:
  * Sets werden in der Reihenfolge angelegt, in der Fremdschluessel sie brauchen.
  * Je Satz: existiert der Schluessel auf dem Ziel, wird PATCH gesendet, sonst POST.
    Saetze werden aktiv angelegt (IsActiveEntity=true), es entstehen keine Entwuerfe.
  * GUIDs bleiben erhalten, damit Fremdschluessel zwischen Sets stimmen.
  * Fremdschluessel, die auf dem Ziel anders heissen (z. B. `status_ID` GUID in
    der Quelle, `status_code` mit Code-Schluessel auf dem Ziel), werden ueber den
    Quelldatensatz uebersetzt: Quell-ID -> Schluesselwert der Zielentitaet.
  * Fremdschluessel `..._ID` duerfen statt einer GUID den Namen (`name`) des
    referenzierten Satzes enthalten, z. B. `"auftrag_ID": "1000123"`, oder einen
    Pfad ueber die Eltern, z. B. `"operation_ID": "1000123/0010"`. Gesucht wird
    zuerst in den Importdateien, dann auf dem Ziel. Der Treffer muss eindeutig sein.
  * Saetze ohne `ID` erhalten eine feste GUID aus Set, Name und Fremdschluesseln.
    Ein erneuter Import findet sie wieder und legt keine Duplikate an.
  * Fehler je Satz werden gesammelt, der Lauf geht weiter. Exit-Code 1 bei Fehlern.

Verwendung:
    python3 scripts/odata-import.py --base-url http://ziel:3034/Service --from export-dir --dry-run
    python3 scripts/odata-import.py --base-url http://ziel:3034/Service --from export-dir
"""

import argparse
import json
import os
import re
import sys
import urllib.parse
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from odata_common import Client, DRAFT_PROPERTIES, Model, ODataError, quote  # noqa: E402

MAX_REPORTED_ERRORS = 20
GUID = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
NAME_PROPERTY = "name"
AUDIT_KEYS = {"Owner_ID", "CreatedBy_ID", "ChangedBy_ID"}
# Fester Namensraum: dieselbe Eingabe ergibt bei jedem Lauf dieselbe GUID.
ID_NAMESPACE = uuid.UUID("6f1c2a7e-3d4b-5e8f-9a0b-1c2d3e4f5a6b")


def load_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


class Report:
    def __init__(self):
        self.errors = []
        self.per_set = {}

    def count(self, set_name, what):
        stats = self.per_set.setdefault(set_name, {"created": 0, "updated": 0, "unchanged": 0, "errors": 0})
        stats[what] += 1

    def error(self, set_name, message):
        self.count(set_name, "errors")
        self.errors.append(f"{set_name}: {message}")


def set_for_type(model, type_name):
    return next((s for s, t in model.sets.items() if t == type_name), None)


def translate_foreign_keys(target_type, target_model, source_record, source_index, set_name, report, body):
    """Fehlende Fremdschluessel aus dem Quelldatensatz ableiten (`{nav}_ID` -> Zielspalte)."""
    for nav_name, nav in target_type.navigations.items():
        if not nav["to_one"] or not nav["refs"]:
            continue
        for prop, referenced in nav["refs"]:
            if prop in body:
                continue
            src_fk = f"{nav_name}_ID"
            src_value = source_record.get(src_fk)
            if src_value is None:
                continue
            parent_set = set_for_type(target_model, nav["target"])
            parent = source_index.get(parent_set, {}).get(str(src_value))
            if parent is None:
                report.error(set_name, f"{source_record.get('ID', '?')}: {src_fk}={src_value} nicht in der Quelle")
                continue
            if referenced not in parent:
                report.error(set_name, f"{source_record.get('ID', '?')}: Zielspalte {referenced} fehlt im Quellsatz")
                continue
            body[prop] = parent[referenced]


class NameResolver:
    """Loest Namen in `..._ID`-Fremdschluesseln zur GUID des referenzierten Satzes auf.

    Ein Pfad `Elternname/Name` (beliebig tief) waehlt unter gleichnamigen Saetzen
    den, dessen Fremdschluessel auf einen Elternsatz mit diesem Namen zeigt.
    """

    def __init__(self, client, model, source_index):
        self.client = client
        self.model = model
        self.source_index = source_index
        self.cache = {}

    def resolve(self, target_set, path):
        key = (target_set, path)
        if key not in self.cache:
            ids = self._ids(target_set, path)
            if not ids:
                raise ValueError(f"{target_set}: kein Satz mit {NAME_PROPERTY}={path!r}")
            if len(ids) > 1:
                raise ValueError(f"{target_set}: {NAME_PROPERTY}={path!r} ist nicht eindeutig ({len(ids)} Saetze)")
            self.cache[key] = ids.pop()
        return self.cache[key]

    def _ids(self, target_set, path):
        target_type = self.model.set_type(target_set)
        if NAME_PROPERTY not in target_type.properties:
            return set()
        prefix, _, name = path.rpartition("/")
        candidates = self._named(target_set, target_type, name)
        if not prefix:
            return set(candidates)
        return {
            cid for cid, rec in candidates.items()
            if any(rec.get(prop) in self._ids(parent_set, prefix)
                   for prop, parent_set in foreign_keys(target_type, self.model))
        }

    def _named(self, target_set, target_type, name):
        """ID -> Satz aller Saetze mit diesem Namen, zuerst aus den Importdateien, sonst vom Ziel."""
        found = {
            str(r["ID"]): r for r in self.source_index.get(target_set, {}).values()
            if r.get(NAME_PROPERTY) == name
        }
        if found:
            return found
        condition = f"{NAME_PROPERTY} eq {quote(name)}"
        if target_type.draft:
            condition += " and IsActiveEntity eq true"
        select = ",".join(["ID"] + [prop for prop, _ in foreign_keys(target_type, self.model)])
        query = urllib.parse.urlencode({"$filter": condition, "$select": select}, quote_via=urllib.parse.quote)
        _, page = self.client.request("GET", f"/{target_set}?{query}")
        return {str(r["ID"]): r for r in (page or {}).get("value", [])}


def foreign_keys(entity_type, model):
    """(Property, Zielset) der `..._ID`-Fremdschluessel auf GUID-Schluessel, ohne Verwaltungsfelder."""
    result = []
    for nav in entity_type.navigations.values():
        if not nav["to_one"]:
            continue
        target_set = set_for_type(model, nav["target"])
        for prop, referenced in nav["refs"]:
            if target_set and referenced == "ID" and prop.endswith("_ID") and prop not in AUDIT_KEYS:
                result.append((prop, target_set))
    return result


def resolve_names(target_type, model, resolver, body):
    """Nicht-GUID-Werte in `..._ID`-Fremdschluesseln ueber den Namen des Ziels aufloesen."""
    for prop, target_set in foreign_keys(target_type, model):
        value = body.get(prop)
        if isinstance(value, str) and value and not GUID.match(value):
            body[prop] = resolver.resolve(target_set, value)


def derived_id(set_name, target_type, model, body):
    """Feste GUID fuer Saetze ohne ID: aus Set, Name und (aufgeloesten) Fremdschluesseln."""
    if body.get(NAME_PROPERTY) not in (None, ""):
        basis = {"name": body[NAME_PROPERTY],
                 "fk": {prop: body.get(prop) for prop, _ in foreign_keys(target_type, model)}}
    else:
        basis = {k: v for k, v in body.items() if k != "ID"}
    return str(uuid.uuid5(ID_NAMESPACE, json.dumps([set_name, basis], sort_keys=True, ensure_ascii=False)))


def import_set(client, model, set_name, records, source_index, resolver, report, dry_run):
    target_type = model.set_type(set_name)
    generated = set()
    for record in records:
        body = {k: v for k, v in record.items() if k in target_type.properties and k not in DRAFT_PROPERTIES}
        label = record.get("ID") or record.get(NAME_PROPERTY) or "?"
        try:
            resolve_names(target_type, model, resolver, body)
            translate_foreign_keys(target_type, model, record, source_index, set_name, report, body)
            if target_type.business_keys() == ["ID"] and body.get("ID") in (None, ""):
                body["ID"] = derived_id(set_name, target_type, model, body)
                if body["ID"] in generated:
                    raise ValueError("doppelt in der Importdatei (gleicher Name und gleiche Fremdschluessel)")
                generated.add(body["ID"])
            if any(k not in body for k in target_type.business_keys()):
                report.error(set_name, f"{label}: Schluessel {target_type.keys} fehlt im Satz")
                continue
            if "ID" in body:
                # Aufgeloeste Fassung fuer Namenssuchen der Kind-Sets.
                source_index.setdefault(set_name, {})[str(body["ID"])] = {**record, **body}
            key = target_type.key_expr(body, active=True)
            path = f"/{set_name}({key})"
            status, current = client.request("GET", path, accept_status=(404,))
            if status == 200:
                patch = {k: v for k, v in body.items() if k not in target_type.business_keys()}
                # Nur Abweichungen schreiben (siehe odata-import Rust-Binary).
                if all(current.get(k) == v for k, v in patch.items()):
                    report.count(set_name, "unchanged")
                    continue
                if not dry_run:
                    client.request("PATCH", path, patch)
                report.count(set_name, "updated")
            else:
                if not dry_run:
                    # Der Export enthaelt nur aktive Saetze. Bei Entitaeten mit Drafts wird
                    # direkt aktiv angelegt (kein Entwurf, daher auch kein draftActivate).
                    if target_type.draft:
                        body["IsActiveEntity"] = True
                    client.request("POST", f"/{set_name}", body)
                report.count(set_name, "created")
        except (ODataError, ValueError) as err:
            report.error(set_name, f"{label}: {err}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--user", default="export@local", help="Benutzer fuer den Auth-Header")
    parser.add_argument("--user-header", default="X-Forwarded-User",
                        help="Header, in dem der Benutzer uebergeben wird (Server: AUTH_USER_HEADER)")
    parser.add_argument("--base-url", required=True, help="Service-Wurzel des Ziels")
    parser.add_argument("--from", dest="source", required=True, help="Verzeichnis aus odata-export.py")
    parser.add_argument("--dry-run", action="store_true", help="nur pruefen und uebersetzen, nichts schreiben")
    args = parser.parse_args()

    manifest = load_json(os.path.join(args.source, "manifest.json"))
    if manifest.get("format") != 1:
        parser.error(f"unbekanntes Exportformat: {manifest.get('format')}")
    sources = {s["set"]: s for s in manifest["sets"]}

    client = Client(args.base_url, user=args.user, user_header=args.user_header)
    model = Model(client.metadata_xml())

    sets = [s for s in manifest["order"] if s in model.sets]
    skipped = [s for s in manifest["order"] if s not in model.sets]
    for s in skipped:
        print(f"uebersprungen (Ziel kennt das Set nicht): {s}")
    order = model.order(sets)

    # Quelldaten als Index (Set -> ID -> Satz) fuer die FK-Uebersetzung.
    source_index = {}
    for set_name in sets:
        records = load_json(os.path.join(args.source, sources[set_name]["file"]))
        source_index[set_name] = {str(r["ID"]): r for r in records if r.get("ID") not in (None, "")}

    resolver = NameResolver(client, model, source_index)
    report = Report()
    for set_name in order:
        records = load_json(os.path.join(args.source, sources[set_name]["file"]))
        import_set(client, model, set_name, records, source_index, resolver, report, args.dry_run)
        stats = report.per_set.get(set_name, {"created": 0, "updated": 0, "unchanged": 0, "errors": 0})
        print(f"{set_name:32s} neu {stats['created']:5d}  aktualisiert {stats['updated']:5d}  "
              f"unveraendert {stats['unchanged']:5d}  Fehler {stats['errors']:4d}")

    mode = "Probelauf" if args.dry_run else "Import"
    total_errors = sum(s["errors"] for s in report.per_set.values())
    print(f"{mode} abgeschlossen, {total_errors} Fehler")
    for message in report.errors[:MAX_REPORTED_ERRORS]:
        print(f"  {message}")
    if len(report.errors) > MAX_REPORTED_ERRORS:
        print(f"  ... {len(report.errors) - MAX_REPORTED_ERRORS} weitere")
    return 1 if total_errors else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ODataError, OSError, ValueError, KeyError) as err:
        print(f"Fehler: {err}", file=sys.stderr)
        sys.exit(1)
