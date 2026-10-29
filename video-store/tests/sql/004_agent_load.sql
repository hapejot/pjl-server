-- 004 Agent Load: Pruefungen des Ziels (public."AgentSet") gegen das Staging.

SELECT 'row_count', t.n, s.n
FROM (SELECT count(*) AS n FROM public."AgentSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.agent) s
WHERE t.n <> s.n;

SELECT 'missing_in_target', s."ID"
FROM stage.agent s
LEFT JOIN public."AgentSet" t ON t."ID" = s."ID" AND t."IsActiveEntity"
WHERE t."ID" IS NULL;

SELECT 'bad_timestamp_format', t."ID"
FROM public."AgentSet" t
WHERE t.last_heartbeat !~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}(Z|[+-]\d{2}:\d{2})$'
   OR t.created_at !~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}(Z|[+-]\d{2}:\d{2})$';

SELECT 'timestamp_value', t."ID"
FROM public."AgentSet" t
JOIN stage.agent s ON s."ID" = t."ID"
WHERE t.last_heartbeat::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.last_heartbeat)
   OR t.created_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.created_at);

SELECT 'field_mismatch', t."ID"
FROM public."AgentSet" t
JOIN stage.agent s ON s."ID" = t."ID"
WHERE t.name IS DISTINCT FROM s.name
   OR t.base_url IS DISTINCT FROM s.base_url
   OR t.status IS DISTINCT FROM s.status
   OR t.version IS DISTINCT FROM s.version;

-- Die Video-Zeilen im Ziel verweisen auf einen Agenten im Ziel.
SELECT 'video_agent_not_in_target', v."ID"
FROM public."VideoSet" v
LEFT JOIN public."AgentSet" a ON a."ID" = v."agent_ID" AND a."IsActiveEntity"
WHERE a."ID" IS NULL;
