"""Gemeinsame Hilfen fuer odata-export.py und odata-import.py.

Nur Standardbibliothek. Liest $metadata (EDMX) in ein einfaches Modell:
Entitaetstypen mit Schluessel, Properties und Navigationen (inkl.
ReferentialConstraint), Entitaetssets, sowie eine Reihenfolge, in der die
Sets angelegt werden koennen (Ziele vor Quellen).
"""

import json
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

# Felder, die nur der Entwurfszustand liefert. Beim Export verworfen, beim Import nie gesendet.
DRAFT_PROPERTIES = {
    "IsActiveEntity", "HasActiveEntity", "HasDraftEntity", "DraftAdministrativeData",
    "DraftAdministrativeData_ID", "DraftMessages", "SiblingEntity",
}


class ODataError(RuntimeError):
    def __init__(self, method, path, status, detail):
        super().__init__(f"{method} {path} -> HTTP {status}: {detail}")
        self.status = status


class Client:
    def __init__(self, base_url, timeout=60, user=None, user_header="X-Forwarded-User"):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.user = user
        self.user_header = user_header

    def _headers(self):
        headers = {"Accept": "application/json", "Content-Type": "application/json"}
        if self.user:
            headers[self.user_header] = self.user
        return headers

    def request(self, method, path, body=None, accept_status=()):
        """Liefert (status, json-oder-None). Fehlerstatus ausser `accept_status` werfen."""
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(
            self.base_url + path,
            data=data,
            method=method,
            headers=self._headers(),
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                raw = resp.read()
                text = raw.decode("utf-8").strip()
                return resp.status, (json.loads(text) if text else None)
        except urllib.error.HTTPError as err:
            if err.code in accept_status:
                return err.code, None
            detail = err.read().decode("utf-8", errors="replace")
            raise ODataError(method, path, err.code, detail[:500]) from None

    def metadata_xml(self):
        headers = {"Accept": "application/xml"}
        if self.user:
            headers[self.user_header] = self.user
        req = urllib.request.Request(self.base_url + "/$metadata", headers=headers)
        with urllib.request.urlopen(req, timeout=self.timeout) as resp:
            return resp.read().decode("utf-8")


def quote(value):
    return "'" + str(value).replace("'", "''") + "'"


class EntityType:
    def __init__(self, name):
        self.name = name
        self.keys = []          # Schluesselproperties in Reihenfolge
        self.properties = {}    # Name -> EDM-Typ
        self.navigations = {}   # Name -> {"target": Typname, "to_one": bool, "refs": [(prop, refprop)]}

    @property
    def draft(self):
        return "IsActiveEntity" in self.properties

    def business_keys(self):
        """Schluesselfelder ohne das Entwurfs-Flag `IsActiveEntity`."""
        return [k for k in self.keys if k != "IsActiveEntity"]

    def key_expr(self, record, active=True):
        """OData-Schluesselausdruck fuer einen Datensatz, z. B. `'planned'` oder `ID=..,IsActiveEntity=true`."""
        keys = self.business_keys()
        if keys == ["ID"]:
            if self.draft:
                return f"ID={record['ID']},IsActiveEntity={'true' if active else 'false'}"
            return str(record["ID"])
        if len(keys) == 1:
            value = record[keys[0]]
            if isinstance(value, str):
                return quote(value)
            if isinstance(value, bool):
                return "true" if value else "false"
            return str(value)
        raise ValueError(f"{self.name}: zusammengesetzter Schluessel {keys} wird nicht unterstuetzt")


class Model:
    def __init__(self, xml_text):
        root = ET.fromstring(xml_text)
        self.types = {}
        self.sets = {}  # Setname -> Typname (ohne Namespace)
        for el in root.iter():
            tag = strip_ns(el.tag)
            if tag == "EntityType":
                self.types[el.get("Name")] = self._parse_type(el)
            elif tag == "EntitySet":
                self.sets[el.get("Name")] = strip_type(el.get("EntityType"))
        self._check_targets()

    def _parse_type(self, el):
        t = EntityType(el.get("Name"))
        for child in el:
            tag = strip_ns(child.tag)
            if tag == "Key":
                t.keys = [p.get("Name") for p in child if strip_ns(p.tag) == "PropertyRef"]
            elif tag == "Property":
                t.properties[child.get("Name")] = child.get("Type")
            elif tag == "NavigationProperty":
                typ = child.get("Type")
                collection = typ.startswith("Collection(")
                target = strip_type(typ)
                refs = [
                    (c.get("Property"), c.get("ReferencedProperty"))
                    for c in child if strip_ns(c.tag) == "ReferentialConstraint"
                ]
                t.navigations[child.get("Name")] = {
                    "target": target, "to_one": not collection, "refs": refs,
                }
        return t

    def _check_targets(self):
        for t in self.types.values():
            for name, nav in t.navigations.items():
                if nav["target"] not in self.types:
                    raise ValueError(f"{t.name}.{name}: Ziel '{nav['target']}' fehlt in $metadata")

    def set_type(self, set_name):
        return self.types[self.sets[set_name]]

    def order(self, set_names):
        """Reihenfolge der Sets: ein Set kommt nach allen Sets, auf die es per Fremdschluessel zeigt.

        Selbstreferenzen werden ignoriert. Zyklen sind ein Fehler.
        """
        set_names = list(set_names)
        deps = {}
        by_type = {v: k for k, v in self.sets.items()}
        for s in set_names:
            t = self.set_type(s)
            deps[s] = {
                by_type[nav["target"]]
                for nav in t.navigations.values()
                if nav["to_one"] and nav["refs"] and nav["target"] in by_type
                and by_type[nav["target"]] != s and by_type[nav["target"]] in set_names
            }
        ordered, remaining = [], dict(deps)
        while remaining:
            ready = sorted(s for s, d in remaining.items() if not d)
            if not ready:
                raise ValueError(f"zyklische Abhaengigkeit zwischen Sets: {sorted(remaining)}")
            for s in ready:
                ordered.append(s)
                del remaining[s]
            for d in remaining.values():
                d.difference_update(ready)
        return ordered


def strip_ns(tag):
    return tag.rsplit("}", 1)[-1]


def strip_type(name):
    return name.split(".", 1)[-1].replace("Collection(", "").rstrip(")") if name else name
