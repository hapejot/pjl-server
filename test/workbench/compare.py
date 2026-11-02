import difflib
import sys
import urllib.request
from pathlib import Path
from xml.dom import minidom



def pretty(data: bytes) -> list[str]:
    xml = minidom.parseString(data).toprettyxml(indent="  ")
    return [line + "\n" for line in xml.splitlines() if line.strip()]

def json_pretty(data: bytes) -> list[str]:
    (lines,) = data.decode("utf-8", errors="replace").splitlines(keepends=False),
    return [line + "\n" for line in lines]


def load_and_compare(url, target):
    with urllib.request.urlopen(url, timeout=10) as resp:
        new = json_pretty(resp.read())
    if not target.exists():
        target.write_text("".join(new), encoding="utf-8")
        print(f"{target} wurde gespeichert.")
        return
    old = json_pretty(target.read_bytes())
    diff = list(difflib.unified_diff(old, new, fromfile=str(target), tofile=url))

    if not diff:
        print(f"{target}: Keine Änderungen.")
        return

    sys.stdout.writelines(diff)
    print(f"\nUnterschiede gefunden. {target} wurde nicht verändert.")

def xml_load_and_compare(url, target):
    with urllib.request.urlopen(url, timeout=10) as resp:
        new = pretty(resp.read())

    if not target.exists():
        target.write_text("".join(new), encoding="utf-8")
        print(f"{target} wurde gespeichert.")
        return

    old = pretty(target.read_bytes())
    diff = list(difflib.unified_diff(old, new, fromfile=str(target), tofile=url))

    if not diff:
        print(f"{target}: Keine Änderungen.")
        return

    sys.stdout.writelines(diff)
    print(f"\nUnterschiede gefunden. {target} wurde nicht verändert.")


def main():
    xml_load_and_compare("http://localhost:8091/Service/$metadata", Path("metadata.xml"))
    load_and_compare("http://localhost:8091/flp/site.json", Path("site.json"))
    apps = ["Appointment", "AppointmentDetail",
            "Article", "ArticleDetail",
            "Contact", "ContactDetail",
            "Customer", "CustomerDetail",
            "DevObject" , "DevObjectDetail",
            "Note", "NoteDetail",
            "Project", "ProjectDetail",
            "Release", "ReleaseDetail",
            "SystemUser", "SystemUserDetail",
            "Task", "TaskDetail"]
    for app in apps:
        load_and_compare(f"http://localhost:8091/apps/{app}/manifest.json", Path(f"{app}-manifest.json"))

if __name__ == "__main__":
    main()
