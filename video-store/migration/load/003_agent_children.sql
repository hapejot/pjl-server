-- Load Agent-Kinder: stage -> public."AgentCapabilitySet", "AgentSettingSet",
-- "AgentVideoDirectorySet". Runtime-Format (siehe 001_videoset.sql).
-- Lauf nur auf leeren Zielen. Voraussetzung: 002_agent.sql (AgentSet) ist geladen,
-- sonst scheitert der Fremdschluessel.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."AgentCapabilitySet")
       OR EXISTS (SELECT 1 FROM public."AgentSettingSet")
       OR EXISTS (SELECT 1 FROM public."AgentVideoDirectorySet") THEN
        RAISE EXCEPTION 'Agent-Kinder sind nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

SET LOCAL TIME ZONE 'Europe/Berlin';

INSERT INTO public."AgentCapabilitySet" (
    "ID", "IsActiveEntity", name, job_type, schema_version,
    parameter_schema, result_schema, requires_video,
    registered_at, updated_at, "agent_ID"
)
SELECT
    s."ID", true, s.name, s.job_type, s.schema_version,
    s.parameter_schema::text, s.result_schema::text, s.requires_video,
    stage.odata_ts(s.registered_at), stage.odata_ts(s.updated_at), s.agent_id
FROM stage.agent_capability s;

INSERT INTO public."AgentSettingSet" (
    "ID", "IsActiveEntity", name, scan_interval_secs, heartbeat_interval_secs,
    updated_at, thumbnail_dir, max_concurrent_jobs, job_poll_interval_seconds,
    max_job_history, "agent_ID"
)
SELECT
    s."ID", true, s.name, s.scan_interval_secs, s.heartbeat_interval_secs,
    stage.odata_ts(s.updated_at), s.thumbnail_dir, s.max_concurrent_jobs,
    s.job_poll_interval_seconds, s.max_job_history, s.agent_id
FROM stage.agent_setting s;

INSERT INTO public."AgentVideoDirectorySet" (
    "ID", "IsActiveEntity", name, path, recursive, created_at, "agent_ID"
)
SELECT
    s."ID", true, s.name, s.path, s.recursive, stage.odata_ts(s.created_at), s.agent_id
FROM stage.agent_video_directory s;
