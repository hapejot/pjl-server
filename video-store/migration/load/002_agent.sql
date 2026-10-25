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
    "ID", "IsActiveEntity", name, hostname, base_url, platform,
    status, last_heartbeat, created_at, version
)
SELECT
    s."ID", true, s.name, s.hostname, s.base_url, s.platform,
    s.status, stage.odata_ts(s.last_heartbeat), stage.odata_ts(s.created_at), s.version
FROM stage.agent s;
