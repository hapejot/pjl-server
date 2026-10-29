-- 000 Schluessel: deterministische UUID-v5-Funktion fuer Nicht-UUID-Schluessel.
--
-- Die Runtime erwartet UUIDs als ID. Bei Text- oder Zahlen-Schluesseln wuerde sie
-- bei jedem Lesen eine neue UUID vergeben, Filter auf ID liefen ins Leere. Der
-- bisherige Import (src/import.rs, derive) loest das ueber UUID v5 aus Namespace,
-- Quelltabelle und Schluessel. Dieselbe Funktion steht hier in SQL, damit
-- Staging und Ziel ohne Mapping-Tabelle identische IDs erzeugen.
--
-- stage.uuid_v5('scenes', '3963451') = 4f98c183-aac2-5827-8aef-fcd30619bde4
-- (identisch zu Python uuid5 und zu derive() in Rust).

CREATE EXTENSION IF NOT EXISTS pgcrypto SCHEMA stage;

CREATE OR REPLACE FUNCTION stage.uuid_v5(tbl text, key text)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
AS $$
DECLARE
    ns bytea := decode('6b1f0c3e9d2a4f7b8e512a9c7d04e1b3', 'hex');
    b  bytea;
    h  text;
BEGIN
    IF key IS NULL THEN
        RETURN NULL;
    END IF;
    b := substring(stage.digest(ns || convert_to(tbl || chr(31) || key, 'UTF8'), 'sha1') FROM 1 FOR 16);
    b := set_byte(b, 6, (get_byte(b, 6) & 15) | 80);
    b := set_byte(b, 8, (get_byte(b, 8) & 63) | 128);
    h := encode(b, 'hex');
    RETURN substr(h,1,8) || '-' || substr(h,9,4) || '-' || substr(h,13,4) || '-'
        || substr(h,17,4) || '-' || substr(h,21,12);
END
$$;
