-- 001 VideoSet: Pruefungen gegen die Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

-- Anzahl muss gleich sein (keine Zeile verloren oder dupliziert).
SELECT 'row_count', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.video_set) s,
     (SELECT count(*) AS n FROM src.videos) q
WHERE s.n <> q.n;

-- Summen der Groessen und Dauern stimmen ueberein.
SELECT 'sum_file_size', s.t, q.t
FROM (SELECT sum(file_size) AS t FROM stage.video_set) s,
     (SELECT sum(file_size) AS t FROM src.videos) q
WHERE s.t IS DISTINCT FROM q.t;

SELECT 'sum_duration', round(s.t::numeric, 3), round(q.t::numeric, 3)
FROM (SELECT sum(duration_seconds) AS t FROM stage.video_set) s,
     (SELECT sum(duration_seconds) AS t FROM src.videos) q
WHERE round(s.t::numeric, 3) IS DISTINCT FROM round(q.t::numeric, 3);

-- Jede Quell-ID ist im Ziel vorhanden, und umgekehrt.
SELECT 'missing_in_stage', v.id::text
FROM src.videos v
LEFT JOIN stage.video_set s ON s."ID" = v.id::text
WHERE s."ID" IS NULL;

SELECT 'extra_in_stage', s."ID"
FROM stage.video_set s
LEFT JOIN src.videos v ON v.id::text = s."ID"
WHERE v.id IS NULL;

-- Zeitstempel: keine Verschiebung, Nullwerte unveraendert.
SELECT 'timestamp_mismatch', v.id::text
FROM src.videos v
JOIN stage.video_set s ON s."ID" = v.id::text
WHERE s.created_at IS DISTINCT FROM v.created_at
   OR s.updated_at IS DISTINCT FROM v.updated_at
   OR s.last_seen IS DISTINCT FROM v.last_seen
   OR s.deleted_at IS DISTINCT FROM v.deleted_at;

-- Fachliche Felder: Abweichungen sind Fehler.
SELECT 'field_mismatch', v.id::text
FROM src.videos v
JOIN stage.video_set s ON s."ID" = v.id::text
WHERE s.file_path IS DISTINCT FROM v.file_path
   OR s.name IS DISTINCT FROM v.file_name
   OR s.agent_id IS DISTINCT FROM v.agent_id::text
   OR s.scene_id IS DISTINCT FROM stage.uuid_v5('scenes', v.scene_id)
   OR s.sha256_checksum IS DISTINCT FROM v.sha256_checksum
   OR s.is_reviewed IS DISTINCT FROM v.is_reviewed;

-- Alle aktiven Instanzen (Draft-Modell) sind gesetzt.
SELECT 'not_active', s."ID"
FROM stage.video_set s
WHERE s."IsActiveEntity" IS NOT TRUE;

-- Eindeutigkeit des Quellschluessels (agent, file_path) bleibt erhalten.
SELECT 'dup_agent_path', s.agent_id, s.file_path
FROM stage.video_set s
GROUP BY s.agent_id, s.file_path
HAVING count(*) > 1;
