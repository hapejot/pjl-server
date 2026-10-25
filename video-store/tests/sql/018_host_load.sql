-- 018 Host Load: Pruefungen des Ziels gegen das Staging.

SELECT 'row_count_host', t.n, s.n
FROM (SELECT count(*) AS n FROM public."HostSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.host) s
WHERE t.n <> s.n;

SELECT 'host_field_mismatch', t."ID"
FROM public."HostSet" t JOIN stage.host s ON s."ID" = t."ID"
WHERE t.hostname IS DISTINCT FROM s.hostname
   OR t.platform IS DISTINCT FROM s.platform
   OR t.name IS DISTINCT FROM s.name;

-- Agenten im Ziel verweisen auf einen Host im Ziel (n:1).
SELECT 'agent_host_not_in_target', a."ID", a."host_ID"
FROM public."AgentSet" a
LEFT JOIN public."HostSet" h ON h."ID" = a."host_ID" AND h."IsActiveEntity"
WHERE h."ID" IS NULL;

SELECT 'agent_host_mismatch', a."ID"
FROM public."AgentSet" a JOIN stage.agent s ON s."ID" = a."ID"
WHERE a."host_ID" IS DISTINCT FROM s.host_id;
