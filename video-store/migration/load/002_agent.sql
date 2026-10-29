-- Load Agent: stage.agent -> public."AgentSet" (Runtime-Format, siehe 001_videoset.sql).
-- Lauf nur auf leerem Ziel.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."AgentSet") THEN
        RAISE EXCEPTION 'public.AgentSet ist nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

SET LOCAL TIME ZONE 'Europe/Berlin';

INSERT INTO public."AgentSet" (
    "ID", "IsActiveEntity", name, base_url,
    status, last_heartbeat, created_at, version, "host_ID"
)
SELECT
    s."ID", true, s.name, s.base_url,
    s.status, stage.odata_ts(s.last_heartbeat), stage.odata_ts(s.created_at), s.version,
    s.host_id
FROM stage.agent s;
