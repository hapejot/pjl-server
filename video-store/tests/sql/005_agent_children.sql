-- 005 Agent-Kinder: Pruefungen gegen die Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

-- Zeilenzahlen.
SELECT 'row_count_capability', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.agent_capability) s,
     (SELECT count(*) AS n FROM src.agent_capabilities) q
WHERE s.n <> q.n;

SELECT 'row_count_setting', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.agent_setting) s,
     (SELECT count(*) AS n FROM src.agent_settings) q
WHERE s.n <> q.n;

SELECT 'row_count_video_directory', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.agent_video_directory) s,
     (SELECT count(*) AS n FROM src.agent_video_directories) q
WHERE s.n <> q.n;

-- Felder: Capabilities (inkl. jsonb-Inhalt).
SELECT 'field_mismatch_capability', c.id::text
FROM src.agent_capabilities c
JOIN stage.agent_capability s ON s."ID" = c.id::text
WHERE s.job_type IS DISTINCT FROM c.job_type
   OR s.schema_version IS DISTINCT FROM c.schema_version
   OR s.parameter_schema IS DISTINCT FROM c.parameter_schema
   OR s.result_schema IS DISTINCT FROM c.result_schema
   OR s.requires_video IS DISTINCT FROM c.requires_video
   OR s.registered_at IS DISTINCT FROM c.registered_at
   OR s.updated_at IS DISTINCT FROM c.updated_at
   OR s.agent_id IS DISTINCT FROM c.agent_id::text;

-- Felder: Settings.
SELECT 'field_mismatch_setting', q.id::text
FROM src.agent_settings q
JOIN stage.agent_setting s ON s."ID" = q.id::text
WHERE s.scan_interval_secs IS DISTINCT FROM q.scan_interval_secs
   OR s.heartbeat_interval_secs IS DISTINCT FROM q.heartbeat_interval_secs
   OR s.updated_at IS DISTINCT FROM q.updated_at
   OR s.thumbnail_dir IS DISTINCT FROM q.thumbnail_dir
   OR s.max_concurrent_jobs IS DISTINCT FROM q.max_concurrent_jobs
   OR s.job_poll_interval_seconds IS DISTINCT FROM q.job_poll_interval_seconds
   OR s.max_job_history IS DISTINCT FROM q.max_job_history
   OR s.agent_id IS DISTINCT FROM q.agent_id::text;

-- Felder: Video-Verzeichnisse.
SELECT 'field_mismatch_video_directory', q.id::text
FROM src.agent_video_directories q
JOIN stage.agent_video_directory s ON s."ID" = q.id::text
WHERE s.path IS DISTINCT FROM q.path
   OR s.recursive IS DISTINCT FROM q.recursive
   OR s.created_at IS DISTINCT FROM q.created_at
   OR s.agent_id IS DISTINCT FROM q.agent_id::text;

-- Jeder Kind-Eintrag verweist auf einen Agenten im Staging.
SELECT 'orphan_child', x.kind, x.id
FROM (
    SELECT 'capability' AS kind, "ID" AS id, agent_id FROM stage.agent_capability
    UNION ALL SELECT 'setting', "ID", agent_id FROM stage.agent_setting
    UNION ALL SELECT 'video_directory', "ID", agent_id FROM stage.agent_video_directory
) x
LEFT JOIN stage.agent a ON a."ID" = x.agent_id
WHERE a."ID" IS NULL;
