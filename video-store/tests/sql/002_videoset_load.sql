-- 002 VideoSet Load: Pruefungen des Ziels (public."VideoSet") gegen das Staging.
-- Jede Abfrage gibt nur bei Fehlern Zeilen aus.

-- Anzahl und Summe stimmen mit dem Staging ueberein.
SELECT 'row_count', t.n, s.n
FROM (SELECT count(*) AS n FROM public."VideoSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.video_set) s
WHERE t.n <> s.n;

-- Jede ID aus dem Staging ist im Ziel (aktive Instanz), und umgekehrt.
SELECT 'missing_in_target', s."ID"
FROM stage.video_set s
LEFT JOIN public."VideoSet" t ON t."ID" = s."ID" AND t."IsActiveEntity"
WHERE t."ID" IS NULL;

SELECT 'extra_in_target', t."ID"
FROM public."VideoSet" t
LEFT JOIN stage.video_set s ON s."ID" = t."ID"
WHERE t."IsActiveEntity" AND s."ID" IS NULL;

-- Zeitstempel im Runtime-Format: ISO 8601 mit Millisekunden und Offset.
SELECT 'bad_timestamp_format', t."ID", t.created_at
FROM public."VideoSet" t
WHERE t.created_at !~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}(Z|[+-]\d{2}:\d{2})$'
   OR (t.deleted_at IS NOT NULL AND t.deleted_at !~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}(Z|[+-]\d{2}:\d{2})$');

-- Der Zeitpunkt bleibt erhalten (auf Millisekunden gekuerzt).
SELECT 'timestamp_value', t."ID"
FROM public."VideoSet" t
JOIN stage.video_set s ON s."ID" = t."ID"
WHERE t.created_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.created_at)
   OR t.updated_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.updated_at)
   OR t.last_seen::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.last_seen)
   OR t.deleted_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.deleted_at);

-- Dauer als Text: gleicher Zahlenwert wie im Staging.
SELECT 'duration_value', t."ID", t.duration_seconds, s.duration_seconds
FROM public."VideoSet" t
JOIN stage.video_set s ON s."ID" = t."ID"
WHERE t.duration_seconds::double precision IS DISTINCT FROM s.duration_seconds;

-- Beziehungsschluessel und fachliche Felder.
SELECT 'field_mismatch', t."ID"
FROM public."VideoSet" t
JOIN stage.video_set s ON s."ID" = t."ID"
WHERE t."agent_ID" IS DISTINCT FROM s.agent_id
   OR t."scene_ID" IS DISTINCT FROM s.scene_id
   OR t.name IS DISTINCT FROM s.name
   OR t.is_reviewed IS DISTINCT FROM s.is_reviewed;
