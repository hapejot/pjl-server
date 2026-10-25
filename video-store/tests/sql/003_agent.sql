-- 003 Agent: Pruefungen gegen die Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

SELECT 'row_count', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.agent) s,
     (SELECT count(*) AS n FROM src.agents) q
WHERE s.n <> q.n;

SELECT 'missing_in_stage', a.id::text
FROM src.agents a
LEFT JOIN stage.agent s ON s."ID" = a.id::text
WHERE s."ID" IS NULL;

SELECT 'field_mismatch', a.id::text
FROM src.agents a
JOIN stage.agent s ON s."ID" = a.id::text
WHERE s.name IS DISTINCT FROM a.name
   OR s.hostname IS DISTINCT FROM a.hostname
   OR s.base_url IS DISTINCT FROM a.base_url
   OR s.platform IS DISTINCT FROM a.platform
   OR s.status IS DISTINCT FROM a.status::text
   OR s.last_heartbeat IS DISTINCT FROM a.last_heartbeat
   OR s.created_at IS DISTINCT FROM a.created_at
   OR s.version IS DISTINCT FROM a.version;

-- Jede Video-Zeile verweist auf einen vorhandenen Agenten (Referenzintegritaet
-- vor dem Load, weil das Ziel dafuer keine Fremdschluessel hat).
SELECT 'video_without_agent', v.id::text
FROM src.videos v
LEFT JOIN stage.agent a ON a."ID" = v.agent_id::text
WHERE a."ID" IS NULL;
