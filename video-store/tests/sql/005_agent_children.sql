-- 005 Agent-Kinder: Pruefungen gegen die Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

-- Zeilenzahlen.
SELECT 'row_count_capability', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.agent_capability) s,
     (SELECT count(*) AS n FROM src.agent_capabilities) q
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

-- Einstellungen: jede Quellzeile ergibt sechs Schluessel-Wert-Paare.
SELECT 'row_count_setting_kv', s.n, 6 * q.n
FROM (SELECT count(*) AS n FROM stage.agent_setting) s,
     (SELECT count(*) AS n FROM src.agent_settings) q
WHERE s.n <> 6 * q.n;

-- Jeder Wert ist inhaltlich gleich dem Quellfeld (JSON-Vergleich).
SELECT 'setting_value', q.agent_id::text, kv.key
FROM src.agent_settings q
CROSS JOIN LATERAL jsonb_each(jsonb_build_object(
    'scan_interval_secs',         q.scan_interval_secs,
    'heartbeat_interval_secs',    q.heartbeat_interval_secs,
    'thumbnail_dir',              q.thumbnail_dir,
    'max_concurrent_jobs',        q.max_concurrent_jobs,
    'job_poll_interval_seconds',  q.job_poll_interval_seconds,
    'max_job_history',            q.max_job_history)) AS kv(key, value)
JOIN stage.agent_setting s ON s.agent_id = q.agent_id::text AND s."key" = kv.key
WHERE s."value"::jsonb IS DISTINCT FROM kv.value;

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
