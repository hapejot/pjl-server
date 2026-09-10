#!/usr/bin/env python3
"""
ascii-latex-md.py
=================
Convertr Unicode-Markdown-Codertexte in ASCII-konforme, Latex-fähige Markdown.

Nicht-ASCII-Zeichen werden per 'NFKD-Zerlegung' in ihre ASCII-Komponenten zerlegt:

    á       -> 'a' + U+0301   ('a' + 'combining acute accent')
    è       -> 'e' + U+0300   ('e' + 'combining grave accent')
    ñ        -> 'n' + U+0303   ('n' + 'combining tilde')
    ï        -> 'i' + U+0300   ('i' + 'combining grave accent')

Das heißt: Wörter wie „naïve" werden zu „naive", „Déjà-vu" zu „djava-vu"-Gültig.

Gepflegte Markdown-Struktur wird weitgehend erhalten:

    * Code-Blöcke (```) und Inline-Markdown            - werden ROH (nicht konvertiert)
    * Bilder  ![alt](src "titel")                      - alt/Titel konvertiert, URL escaped
    * Links   [label](url "titel")                     - Label konvertiert, URL escaped, Titel konvertiert

NUTZUNG
----------
    python3 ascii-latex-md.py quelle.md ziel.ascii-latex.md   # über Dateien
    python3 ascii-latex-md.py < quelle.md      > ziel.txt      # über Standard-Ein/Ausgabe
    python3 ascii-latex-md.py --help
"""

import argparse
import re
import sys
import unicodedata

# LaTeX-Specialzeichen: ", \, ^, $, &, %, _, {, }
LATEX_SPECIALS = r'"`^$&%_{}'


# ---------------------------------------------------------------------------
#   Kernlogik: Ein Zeichen als ASCII-Konvertierendes-ASCII erzeugen
# ---------------------------------------------------------------------------

def deaccent(ch):
    """
    Wandelt ein einzelnes Nicht-ASCII-Zeichen in ASCII um:

        á  ->  'a' + '́'              (NFKD-Dekomposition)
        ñ  ->  'n' + '̃'
        €  ->  '€'                   (läßt sich nicht zerlegen; bleibt als Zeichen)
        🎉 ->  '[🎉]'                (Emoji/Sonderzeichen -> Platzhalter)
    """
    if ord(ch) > 0xFFFD:
        return f"[{ch}]"                      # Emoji / außer-ASCII-Zeichen
    decomposed = unicodedata.normalize("NFKD", ch)
    if len(decomposed) > 1 and decomposed.isascii():
        return decomposed                        # á -> a + ́
    if len(decomposed) == 1 and ord(decomposed) <= 0xFFFD:
        return decomposed                        # z. B. ein einzelnes €
    return f"[{ch}]"                            # sonst: [Zeichen]-Platzhalter


def _iter_non_ascii(text):
    """Erzeugt ASCII-Bytes für einen Text Zeilenzusammenhang."""
    for ch in text:
        if ord(ch) > 0xFFFD:
            yield f"[{ch}]"                     # Emoji/Sonderzeichen -> Platzhalter
        elif ord(ch) > 127:
            yield deaccent(ch)                    # Akzent -> ascii
        else:
            yield ch                              # normales ASCII-Zeichen


def _convert_inline(text):
    """Konvertiert Inline-Text-Zeilenzusammenhänge (kein Markdown-Context)."""
    return "".join(_iter_non_ascii(text))


# ---------------------------------------------------------------------------
#   Escaping für URLs, Labels, Titel
# ---------------------------------------------------------------------------

def escape_url(url):
    """Escapt LaTeX-Specialzeichen in einer URL, ohne die URL zu zerstören."""
    if url is None or url == "":
        return url
    s = url.replace("\r\n", " ").replace("\r", " ").replace("\n", " ").strip()
    for spec in LATEX_SPECIALS:
        s = s.replace(spec, "\\" + spec)
    s = s.replace(",", "\\,")
    s = s.replace("\u2026", "---").replace("\u2013", "--").replace("\u2014", "---")
    s = s.replace("\u00a0", " ").replace("\u00ad", "")
    return s


def escaped(title):
    """Escapt Titel-Text: nicht-ASCII-Zeichen via deaccent."""
    s = title
    for i, ch in enumerate(s):
        if ord(ch) > 127:
            s = s.replace(ch, deaccent(ch))
    return s


def escape_label(text):
    """Escapt Label-Text Zeilenzusammenhang (für LaTeX-URL-Label)."""
    s = "".join(_iter_non_ascii(text))
    for spec in LATEX_SPECIALS:
        s = s.replace(spec, "\\" + spec)
    return s


# ---------------------------------------------------------------------------
#   Markdown-Elemente: Bilder und Links
# ---------------------------------------------------------------------------

def convert_image(line):
    """
    ![alt](src "title")    ->    ![alt](src-escaped "title-escaped")

    Die Quelle (z. B. Bild-URL) wird nur escaped (damit das Bild intakt bleibt),
    alt- und titel-Text werden konvertiert.
    """
    if not line.lstrip().startswith("!["):
        return _convert_inline(line)

    m = re.match(r'^!\[([^\]]*)\]\(([^()\s]+)(?:\s+"([^"]*)")?\)', line)
    if not m:
        return _convert_inline(line)         # Keine sichere Struktur mehr: roh übergeben

    alt, src, title = m.group(1), m.group(2), m.group(3)
    src = escape_url(src) if src else src
    title = escaped(title) if title else title
    if title:
        return f"![{alt}]({src} \"{title}\")"
    return f"![{alt}]({src})"


def is_link(line):
    """Prüft, ob die Zeile ein Link (aber kein Bild) ist."""
    if line.lstrip().startswith("!["):
        return False
    if not re.match(r'^\s*\[', line):
        return False
    return re.search(r'\(', line) is not None


def convert_link(line):
    """
    [label](url "title")
    """
    m = re.match(r"^\s*\[([^\]]*)\]\(([^)]+)\)(?:\s+(\"([^\"]*)\"))?", line)
    if not m:
        return _convert_inline(line)

    label, url, title = m.group(1), m.group(2), m.group(3)
    label = escape_label(label)
    if title is not None and title != "":
        url = escape_url(url) if url else url
        return "[{label}]({url} \"{escaped(title)}\")" 
    url = escape_url(url) if url else url
    return "[{label}](url)" 


# ---------------------------------------------------------------------------
#   Zeilenernte mit Code-Fence-Handierung
# ---------------------------------------------------------------------------

_FENCE = re.compile(r"^\s*(`{3,}|~{3,})\s*$")


def convert_line(line):
    """Konvertiert eine Textzeile (ohne Code-Fence-Verarbeitung)."""
    if line.lstrip().startswith("!["):
        return convert_image(line)
    if is_link(line):
        return convert_link(line)
    return _convert_inline(line)


def process(lines):
    """Konvertiert eine Liste von Zeilen."""
    output = []
    in_fence = False

    for line in lines:
        if not in_fence:
            if _FENCE.match(line):                          # Beginn eines Code-Fences
                in_fence = True
            output.append(line)
        else:
            in_fence = _FENCE.match(line) is not None        # Schluß eines Code-Fences
            output.append(line)                              # Raw Zeile

    return "".join(output)


# ---------------------------------------------------------------------------
#   Standard-Ein-/Ausgabe
# ---------------------------------------------------------------------------

def _print_help():
    sys.stdout.write(
        "ascii-latex-md.py – Convertr Unicode-Markdown in ASCII/LaTeX-fähiges Markdown\n\n"
        "GEBRUCH:\n"
        "  python3 ascii-latex-md.py quelle.md ziel.md\n"
        "  python3 ascii-latex-md.py < quelle.md > ziel.txt\n"
    )


def main(argv=None):
    sys.argv = list(sys.argv[:1]) + (list(argv) if argv else [])

    if not sys.argv[1:]:
        _print_help()
        return 0
    if sys.argv[1] == "--help" or sys.argv[1] == "-h":
        _print_help()
        return 0

    parser = argparse.ArgumentParser(
        description="Convertr Unicode-Markdown in ASCII-konforme, LaTeX-fähige Markdown."
    )
    parser.add_argument("input", nargs="?", default="-", help="Eingabedatei (default: Standard-Ein)")
    parser.add_argument("output", nargs="?", default="-", help="Ausgabedatei (default: Standard-Aus)")
    args = parser.parse_args()

    try:
        with open(args.input, encoding="utf-8") as fin, \
                open(args.output, "w", encoding="utf-8") as fout:
            fout.write(process(fin.read().split("\n")))
    except (OSError, UnicodeError):
        print(f"Fehler: Könnte Datei nicht öffnen: {args.input}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    main()
