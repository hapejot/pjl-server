-- 017 Host: Pruefungen Staging gegen Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

-- Ein Host je Hostname (Schluessel kleingeschrieben).
SELECT 'row_count_host', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.host) s,
     (SELECT count(DISTINCT lower(hostname)) AS n FROM src.agents) q
WHERE s.n <> q.n;

-- Ein Hostname hat genau eine Plattform: sonst waere Host.platform mehrdeutig.
SELECT 'platform_conflict', lower(a.hostname), count(DISTINCT a.platform)
FROM src.agents a
GROUP BY lower(a.hostname)
HAVING count(DISTINCT a.platform) > 1;

-- Jeder Agent hat genau den Host seines Hostnamens.
SELECT 'agent_host_mismatch', a."ID"
FROM stage.agent a
JOIN src.agents q ON q.id::text = a."ID"
JOIN stage.host h ON h."ID" = a.host_id
WHERE h.hostname IS DISTINCT FROM lower(q.hostname)
   OR h.platform IS DISTINCT FROM q.platform;

-- Keine Agenten ohne Host.
SELECT 'agent_without_host', a."ID"
FROM stage.agent a
LEFT JOIN stage.host h ON h."ID" = a.host_id
WHERE h."ID" IS NULL;
