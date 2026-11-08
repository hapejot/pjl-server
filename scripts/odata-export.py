#!/usr/bin/env python3
"""Exportiert die Daten eines OData-Services in ein lokales Verzeichnis.

Liest `$metadata` und alle Entitaetssets (nur aktive Saetze, Entwuerfe werden
verworfen). Schreibt pro Set eine JSON-Datei, dazu `metadata.xml` und
`manifest.json` mit Reihenfolge, Schluesseln und Anzahlen. Das Tool schreibt
nichts auf den Server.

Verwendung:
    python3 scripts/odata-export.py --base-url http://localhost:3034/Service --out export-dir
    python3 scripts/odata-export.py --base-url URL --out DIR --sets ProjectStatusSet,ProjectSet

Das Verzeichnis ist die Eingabe fuer scripts/odata-import.py.
"""

import argparse
import datetime
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from odata_common import Client, DRAFT_PROPERTIES, Model, ODataError  # noqa: E402

FORMAT_VERSION = 1
PAGE = 500


def fetch_all(client, set_name):
    records, skip = [], 0
    while True:
        _, page = client.request("GET", f"/{set_name}?$top={PAGE}&$skip={skip}")
        items = (page or {}).get("value", [])
        records.extend(items)
        if len(items) < PAGE:
            return records
        skip += PAGE


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base-url", required=True, help="Service-Wurzel, z. B. http://host:port/Service")
    parser.add_argument("--out", required=True, help="Zielverzeichnis (wird angelegt)")
    parser.add_argument("--sets", help="Komma-getrennte Setnamen (Standard: alle)")
    args = parser.parse_args()

    client = Client(args.base_url)
    xml_text = client.metadata_xml()
    model = Model(xml_text)

    wanted = [s.strip() for s in args.sets.split(",")] if args.sets else sorted(model.sets)
    unknown = [s for s in wanted if s not in model.sets]
    if unknown:
        parser.error(f"unbekannte Sets: {', '.join(unknown)}")
    order = model.order(wanted)

    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, "metadata.xml"), "w", encoding="utf-8") as f:
        f.write(xml_text)

    manifest_sets = []
    total = 0
    for set_name in order:
        etype = model.set_type(set_name)
        raw = fetch_all(client, set_name)
        active = [r for r in raw if r.get("IsActiveEntity") is not False]
        # Nur deklarierte Properties; Navigationen und Entwurfsfelder fallen weg.
        keep = [p for p in etype.properties if p not in DRAFT_PROPERTIES]
        records = [{k: r[k] for k in keep if k in r} for r in active]
        with open(os.path.join(args.out, f"{set_name}.json"), "w", encoding="utf-8") as f:
            json.dump(records, f, ensure_ascii=False, indent=1)
        manifest_sets.append({
            "set": set_name,
            "type": etype.name,
            "keys": etype.keys,
            "draft": etype.draft,
            "count": len(records),
            "file": f"{set_name}.json",
            "navigations": {
                n: {"target_set": next((s for s, t in model.sets.items() if t == nav["target"]), None),
                    "to_one": nav["to_one"], "refs": nav["refs"]}
                for n, nav in etype.navigations.items()
            },
        })
        total += len(records)
        print(f"{set_name:32s} {len(records):6d} Saetze")

    manifest = {
        "format": FORMAT_VERSION,
        "source": args.base_url,
        "exported_at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "order": order,
        "sets": manifest_sets,
    }
    with open(os.path.join(args.out, "manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=1)
    print(f"{len(order)} Sets, {total} Saetze nach {args.out}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ODataError, OSError, ValueError) as err:
        print(f"Fehler: {err}", file=sys.stderr)
        sys.exit(1)
