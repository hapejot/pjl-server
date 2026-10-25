-- 006 Agent-Kinder Load: Pruefungen des Ziels gegen das Staging.

SELECT 'row_count_capability', t.n, s.n
FROM (SELECT count(*) AS n FROM public."AgentCapabilitySet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.agent_capability) s
WHERE t.n <> s.n;

SELECT 'row_count_setting', t.n, s.n
FROM (SELECT count(*) AS n FROM public."AgentSettingSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.agent_setting) s
WHERE t.n <> s.n;

SELECT 'row_count_video_directory', t.n, s.n
FROM (SELECT count(*) AS n FROM public."AgentVideoDirectorySet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.agent_video_directory) s
WHERE t.n <> s.n;

-- Capabilities: jsonb im Ziel als Text, muss inhaltlich gleich sein.
SELECT 'json_mismatch', t."ID"
FROM public."AgentCapabilitySet" t
JOIN stage.agent_capability s ON s."ID" = t."ID"
WHERE t.parameter_schema::jsonb IS DISTINCT FROM s.parameter_schema
   OR t.result_schema::jsonb IS DISTINCT FROM s.result_schema
   OR t.job_type IS DISTINCT FROM s.job_type
   OR t.requires_video IS DISTINCT FROM s.requires_video
   OR t."agent_ID" IS DISTINCT FROM s.agent_id;

-- Zeitstempel im Runtime-Format und Zeitpunkt erhalten.
SELECT 'timestamp_value', t."ID"
FROM public."AgentCapabilitySet" t
JOIN stage.agent_capability s ON s."ID" = t."ID"
WHERE t.registered_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.registered_at)
   OR t.updated_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.updated_at)
   OR t.registered_at !~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}(Z|[+-]\d{2}:\d{2})$';

SELECT 'timestamp_value_setting', t."ID"
FROM public."AgentSettingSet" t
JOIN stage.agent_setting s ON s."ID" = t."ID"
WHERE t.updated_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.updated_at);

SELECT 'timestamp_value_directory', t."ID"
FROM public."AgentVideoDirectorySet" t
JOIN stage.agent_video_directory s ON s."ID" = t."ID"
WHERE t.created_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.created_at);

-- Settings und Verzeichnisse: fachliche Felder und Referenz.
SELECT 'field_mismatch_setting', t."ID"
FROM public."AgentSettingSet" t
JOIN stage.agent_setting s ON s."ID" = t."ID"
WHERE t.scan_interval_secs IS DISTINCT FROM s.scan_interval_secs
   OR t.heartbeat_interval_secs IS DISTINCT FROM s.heartbeat_interval_secs
   OR t.thumbnail_dir IS DISTINCT FROM s.thumbnail_dir
   OR t.max_concurrent_jobs IS DISTINCT FROM s.max_concurrent_jobs
   OR t.job_poll_interval_seconds IS DISTINCT FROM s.job_poll_interval_seconds
   OR t.max_job_history IS DISTINCT FROM s.max_job_history
   OR t."agent_ID" IS DISTINCT FROM s.agent_id;

SELECT 'field_mismatch_video_directory', t."ID"
FROM public."AgentVideoDirectorySet" t
JOIN stage.agent_video_directory s ON s."ID" = t."ID"
WHERE t.path IS DISTINCT FROM s.path
   OR t.recursive IS DISTINCT FROM s.recursive
   OR t."agent_ID" IS DISTINCT FROM s.agent_id;
