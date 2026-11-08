#!/usr/bin/env python3
"""Testdaten fuer das poc-crm ueber die OData-Schnittstelle anlegen.

Erzeugt Firmen, Kontakte und Verkaufschancen. Waehrungen und Mengeneinheiten
werden nur angelegt, falls der Server sie noch nicht enthaelt. Entitaeten mit
Entwurfszustand werden als Entwurf angelegt und anschliessend per
Service.draftActivate aktiviert.

Vorhandene Datensaetze (gleicher Name bzw. Code, bei Kontakten und
Verkaufschancen zusaetzlich gleiche Firma) werden nicht erneut angelegt, das
Script ist also mehrfach ausfuehrbar. Die Daten sind mit --seed reproduzierbar.

Verwendung:
    python3 poc-crm/seed-testdata.py [--base-url URL] [--seed N] [--accounts N]
"""

import argparse
import json
import random
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timedelta

CURRENCIES = [
    {"code": "EUR", "name": "Euro", "decimal_places": 2},
    {"code": "USD", "name": "US-Dollar", "decimal_places": 2},
    {"code": "CHF", "name": "Schweizer Franken", "decimal_places": 2},
    {"code": "JPY", "name": "Yen", "decimal_places": 0},
]

UNITS = [
    {"code": "H", "name": "Stunde", "decimal_places": 0, "dimension": "time"},
    {"code": "D", "name": "Tag", "decimal_places": 0, "dimension": "time"},
    {"code": "PCS", "name": "Stueck", "decimal_places": 0, "dimension": "count"},
    {"code": "KG", "name": "Kilogramm", "decimal_places": 3, "dimension": "mass"},
    {"code": "M", "name": "Meter", "decimal_places": 2, "dimension": "length"},
]

# (Name, Branche, Stadt, Domain fuer Website und E-Mail)
ACCOUNTS = [
    ("Mueller Maschinenbau GmbH", "Maschinenbau", "Stuttgart", "mueller-maschinenbau.example"),
    ("Nordlicht Logistik AG", "Logistik", "Hamburg", "nordlicht-logistik.example"),
    ("Alpenblick Hotels GmbH", "Tourismus", "Innsbruck", "alpenblick-hotels.example"),
    ("Rheinwerk Software GmbH", "Software", "Koeln", "rheinwerk-software.example"),
    ("Baeckerei Sonnenschein e.K.", "Lebensmittel", "Freiburg", "sonnenschein-baeckerei.example"),
    ("Helvetia Pharma AG", "Pharma", "Basel", "helvetia-pharma.example"),
    ("Schwarzwald Holz KG", "Holzindustrie", "Freiburg", "schwarzwald-holz.example"),
    ("Elbe Energie GmbH", "Energie", "Dresden", "elbe-energie.example"),
    ("Kunststoff Weber GmbH", "Kunststoff", "Nuernberg", "kunststoff-weber.example"),
    ("Baltic Shipping Ltd.", "Schifffahrt", "Rostock", "baltic-shipping.example"),
]

FIRST_NAMES = ["Anna", "Jonas", "Lena", "Markus", "Sophie", "Tobias",
               "Julia", "Felix", "Eva", "Lukas", "Mia", "Stefan"]
LAST_NAMES = ["Keller", "Brandt", "Schneider", "Wagner", "Hoffmann",
              "Richter", "Koch", "Fischer", "Becker", "Vogel"]

OPPORTUNITY_TOPICS = ["Rahmenvertrag", "Pilotprojekt", "Lizenzerweiterung",
                      "Wartungsvertrag", "Onboarding", "Beratungsprojekt"]
STAGES = ["Qualifizierung", "Angebot", "Verhandlung", "Abschluss"]
STATUSES = ["open", "open", "won", "lost"]
CURRENCY_CODES = ["EUR", "EUR", "USD", "CHF"]
DURATION_UNITS = ["H", "D"]
QUANTITY_UNITS = ["PCS", "KG", "M"]


class ODataClient:
    def __init__(self, base_url):
        self.base_url = base_url.rstrip("/")

    def request(self, method, path, body=None):
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(
            self.base_url + path,
            data=data,
            method=method,
            headers={"Accept": "application/json", "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                raw = resp.read()
                return json.loads(raw) if raw else None
        except urllib.error.HTTPError as err:
            detail = err.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"{method} {path} -> HTTP {err.code}: {detail}") from None

    def find(self, set_name, odata_filter):
        query = urllib.parse.urlencode({"$filter": odata_filter}, quote_via=urllib.parse.quote)
        return self.request("GET", f"/{set_name}?{query}")["value"]

    def create(self, set_name, body):
        entity = self.request("POST", f"/{set_name}", body)
        if entity.get("IsActiveEntity") is False:
            key = f"ID={entity['ID']},IsActiveEntity=false"
            entity = self.request("POST", f"/{set_name}({key})/Service.draftActivate", {})
        return entity

    def ensure(self, set_name, odata_filter, body):
        """Liefert (Entitaet, neu_angelegt)."""
        existing = self.find(set_name, odata_filter)
        if existing:
            return existing[0], False
        return self.create(set_name, body), True


def quote(value):
    return "'" + value.replace("'", "''") + "'"


def ensure_master_data(client, stats):
    for row in CURRENCIES:
        _, created = client.ensure("CurrencySet", f"code eq {quote(row['code'])}", row)
        stats.record("Waehrung", created)
    for row in UNITS:
        _, created = client.ensure("UnitOfMeasureSet", f"code eq {quote(row['code'])}", row)
        stats.record("Mengeneinheit", created)


def load_status_ids(client):
    """Ordnet jeden Status-Code (open, won, lost) der ID seines Listeneintrags zu."""
    items = client.request("GET", "/OpportunityStageSet")["value"]
    status_ids = {item["value"]: item["ID"] for item in items if item.get("IsActiveEntity", True)}
    missing = [code for code in STATUSES if code not in status_ids]
    if missing:
        raise RuntimeError(f"Werteliste OpportunityStageSet unvollstaendig, fehlt: {missing}")
    return status_ids


def seed_accounts(client, rng, count, stats):
    status_ids = load_status_ids(client)
    for name, industry, city, domain in ACCOUNTS[:count]:
        account, created = client.ensure(
            "AccountSet",
            f"name eq {quote(name)}",
            {"name": name, "industry": industry, "city": city,
             "website": f"https://www.{domain}"},
        )
        stats.record("Firma", created)
        seed_contacts(client, rng, account, domain, stats)
        seed_opportunities(client, rng, account, name, status_ids, stats)


def seed_contacts(client, rng, account, domain, stats):
    for _ in range(rng.randint(1, 3)):
        first, last = rng.choice(FIRST_NAMES), rng.choice(LAST_NAMES)
        name = f"{first} {last}"
        _, created = client.ensure(
            "ContactSet",
            f"name eq {quote(name)} and account_ID eq {account['ID']}",
            {
                "name": name,
                "email": f"{first.lower()}.{last.lower()}@{domain}",
                "phone": f"+49 {rng.randint(30, 89)} {rng.randint(1000000, 9999999)}",
                "account_ID": account["ID"],
            },
        )
        stats.record("Kontakt", created)


def seed_opportunities(client, rng, account, account_name, status_ids, stats):
    for _ in range(rng.randint(1, 3)):
        topic = rng.choice(OPPORTUNITY_TOPICS)
        name = f"{topic} {account_name.split()[0]}"
        duration_unit = rng.choice(DURATION_UNITS)
        _, created = client.ensure(
            "OpportunitySet",
            f"name eq {quote(name)} and account_ID eq {account['ID']}",
            {
                "name": name,
                "account_ID": account["ID"],
                "stage": rng.choice(STAGES),
                "status_ID": status_ids[rng.choice(STATUSES)],
                "currency_code": rng.choice(CURRENCY_CODES),
                "amount": round(rng.uniform(5000, 500000), 2),
                "duration_unit": duration_unit,
                "duration": rng.randint(1, 120 if duration_unit == "D" else 400),
                "unit_code": rng.choice(QUANTITY_UNITS),
                "quantity": round(rng.uniform(1, 500), 3),
                "close_date": (
                    datetime.now().astimezone().date() + timedelta(days=rng.randint(-60, 240))
                ).isoformat(),
            },
        )
        stats.record("Verkaufschance", created)


class Stats:
    def __init__(self):
        self.created = {}
        self.existing = {}

    def record(self, label, created):
        bucket = self.created if created else self.existing
        bucket[label] = bucket.get(label, 0) + 1

    def print(self):
        for label in ["Waehrung", "Mengeneinheit", "Firma", "Kontakt", "Verkaufschance"]:
            print(f"{label:15} neu: {self.created.get(label, 0):3}  vorhanden: {self.existing.get(label, 0):3}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base-url", default="http://localhost:8090/Service",
                        help="OData-Basis-URL (Standard: %(default)s)")
    parser.add_argument("--seed", type=int, default=42, help="Zufalls-Seed fuer reproduzierbare Daten")
    parser.add_argument("--accounts", type=int, default=len(ACCOUNTS),
                        help=f"Anzahl Firmen, max. {len(ACCOUNTS)} (Standard: %(default)s)")
    args = parser.parse_args()

    if not 1 <= args.accounts <= len(ACCOUNTS):
        parser.error(f"--accounts muss zwischen 1 und {len(ACCOUNTS)} liegen")

    client = ODataClient(args.base_url)
    rng = random.Random(args.seed)
    stats = Stats()
    try:
        ensure_master_data(client, stats)
        seed_accounts(client, rng, args.accounts, stats)
    except (RuntimeError, urllib.error.URLError) as err:
        print(f"Fehler: {err}", file=sys.stderr)
        return 1
    stats.print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
