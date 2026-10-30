# Migriert eine ddic.json auf den `ui`-Block (Anzeige-Formate je Atom-Feld).
#
# Aufruf:
#   jq --argjson formats '{"Video":{"duration_seconds":"duration","bitrate":"bitrate"}}' \
#      -f scripts/ddic-migrate-ui.jq video-store/ddic.json > video-store/ddic.new.json
#
# --argjson formats: { "<Entitaet>": { "<Feld>": "<Format>" } }
#   Erlaubte Formate: duration, bytes, bitrate, date, datetime.
#   Unbekannte Entitaeten, Felder oder Formate brechen mit Fehler ab.
#
# Veraenderungen:
#   * Atom-Felder mit Eintrag in $formats bekommen "ui": { "format": "<Format>" }.
#   * Ein vorhandener `ui`-Block bleibt erhalten, nur `format` wird gesetzt.
#   * Alle anderen Felder bleiben unveraendert (link, multiline, side_effects bleiben gueltig).
#   * Die Rohfelder werden nicht in ddic.json ausgeblendet; das uebernimmt der Server
#     (fiori-server/src/ui_ext.rs) aus den ui-Bloecken.

def allowed: ["duration", "bytes", "bitrate", "date", "datetime"];

($formats // {}) as $f
| [ $f | to_entries[] | .key as $e | .value | to_entries[]
    | { entity: $e, field: .key, format: .value } ] as $wanted
| ( [ .entities[].name ] ) as $entities
| ( [ .entities[] | .fields[]? | select(has("Atom")) | .Atom.name ] ) as $atoms
| ( $wanted | map(select(.format as $x | allowed | index($x) == null)) ) as $badfmt
| ( $wanted | map(select(.entity as $x | $entities | index($x) == null)) ) as $noent
| ( $wanted | map(select(.field as $x | $atoms | index($x) == null)) ) as $nofield
| if ($badfmt | length) > 0 then
    error("unbekanntes Format: " + ($badfmt | map("\(.entity).\(.field)=\(.format)") | join(", ")))
  elif ($noent | length) > 0 then
    error("unbekannte Entitaet: " + ($noent | map(.entity) | unique | join(", ")))
  elif ($nofield | length) > 0 then
    error("unbekanntes Atom-Feld: " + ($nofield | map("\(.entity).\(.field)") | join(", ")))
  else . end
| .entities |= map(
    . as $en
    | ($f[$en.name] // {}) as $efmt
    | if ($efmt | length) == 0 then .
      else .fields |= map(
        if has("Atom") and ((.Atom.name) as $n | $efmt | has($n))
        then .Atom.ui = ((.Atom.ui // {}) + { format: $efmt[.Atom.name] })
        else . end)
      end)
