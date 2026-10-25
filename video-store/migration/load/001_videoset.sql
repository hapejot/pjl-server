-- Load VideoSet: stage.video_set (typisiert) -> public."VideoSet" (Runtime-Format).
--
-- Die Runtime speichert DateTimeOffset und Decimal als TEXT (relational-core,
-- Dialekt). Das Format entspricht dem bisherigen Import (import.rs):
-- ISO 8601 mit Millisekunden und Offset, z. B. 2026-05-30T16:52:22.271+02:00.
-- Die Zeitzone ist Europe/Berlin, wie beim bisherigen Import.
--
-- Lauf nur auf leerem Ziel. Eine bereits gefuellte Tabelle wird abgewiesen.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."VideoSet") THEN
        RAISE EXCEPTION 'public.VideoSet ist nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

SET LOCAL TIME ZONE 'Europe/Berlin';

CREATE OR REPLACE FUNCTION stage.odata_ts(t timestamptz)
RETURNS text
LANGUAGE sql
STABLE
AS $$
    SELECT CASE WHEN t IS NULL THEN NULL ELSE
        to_char(t, 'YYYY-MM-DD"T"HH24:MI:SS.MS')
        || CASE WHEN length(to_char(t, 'OF')) = 3
                THEN to_char(t, 'OF') || ':00'
                ELSE to_char(t, 'OF') END
    END
$$;

INSERT INTO public."VideoSet" (
    "ID", "IsActiveEntity", file_path, name, file_size, duration_seconds,
    width, height, codec, bitrate, is_streamable, thumbnail_path,
    created_at, updated_at, last_seen, thumbnail_count, deleted_at,
    pending_physical_delete, title, is_reviewed, sha256_checksum,
    "agent_ID", "scene_ID"
)
SELECT
    s."ID", true, s.file_path, s.name, s.file_size,
    s.duration_seconds::text,
    s.width, s.height, s.codec, s.bitrate, s.is_streamable, s.thumbnail_path,
    stage.odata_ts(s.created_at), stage.odata_ts(s.updated_at),
    stage.odata_ts(s.last_seen), s.thumbnail_count, stage.odata_ts(s.deleted_at),
    s.pending_physical_delete, s.title, s.is_reviewed, s.sha256_checksum,
    s.agent_id, s.scene_id
FROM stage.video_set s;
