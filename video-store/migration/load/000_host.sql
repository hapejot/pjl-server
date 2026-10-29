-- Load Host: stage.host -> public."HostSet" (vor den Agents, siehe 002_agent.sql).
-- Lauf nur auf leerem Ziel.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."HostSet") THEN
        RAISE EXCEPTION 'public.HostSet ist nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

INSERT INTO public."HostSet" ("ID", "IsActiveEntity", name, hostname, platform)
SELECT s."ID", true, s.name, s.hostname, s.platform
FROM stage.host s;
