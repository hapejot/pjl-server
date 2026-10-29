-- 015 Video-Aktivitaet und Einstellungen: Pruefungen Staging gegen Quelle.
-- Jede Abfrage gibt nur bei Fehlern Zeilen aus.

SELECT 'row_count_video_rating', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.video_rating) s, (SELECT count(*) AS n FROM src.video_ratings) q
WHERE s.n <> q.n;

SELECT 'row_count_video_view', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.video_view) s, (SELECT count(*) AS n FROM src.video_views) q
WHERE s.n <> q.n;

SELECT 'row_count_video_thumbnail', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.video_thumbnail) s, (SELECT count(*) AS n FROM src.video_thumbnails) q
WHERE s.n <> q.n;

SELECT 'row_count_video_job', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.video_job) s, (SELECT count(*) AS n FROM src.video_jobs) q
WHERE s.n <> q.n;

SELECT 'row_count_global_thumbnail', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.global_thumbnail_setting) s,
     (SELECT count(*) AS n FROM src.global_thumbnail_settings) q
WHERE s.n <> q.n;

SELECT 'row_count_global_snapshot', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.global_snapshot_position) s,
     (SELECT count(*) AS n FROM src.global_snapshot_positions) q
WHERE s.n <> q.n;

SELECT 'row_count_label_category', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.label_category) s, (SELECT count(*) AS n FROM src.label_categories) q
WHERE s.n <> q.n;

SELECT 'row_count_label_option', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.label_category_option) s,
     (SELECT count(*) AS n FROM src.label_category_options) q
WHERE s.n <> q.n;

-- Feldinhalte.
SELECT 'field_mismatch_video_rating', r.id
FROM src.video_ratings r JOIN stage.video_rating s ON s."ID" = r.id::text
WHERE s.rating IS DISTINCT FROM r.rating OR s.created_at IS DISTINCT FROM r.created_at
   OR s.updated_at IS DISTINCT FROM r.updated_at OR s.video_id IS DISTINCT FROM r.video_id::text;

SELECT 'field_mismatch_video_view', v.id
FROM src.video_views v JOIN stage.video_view s ON s."ID" = v.id::text
WHERE s.watched_at IS DISTINCT FROM v.watched_at
   OR s.watch_duration_seconds IS DISTINCT FROM v.watch_duration_seconds
   OR s.completed IS DISTINCT FROM v.completed OR s.video_id IS DISTINCT FROM v.video_id::text;

SELECT 'field_mismatch_video_thumbnail', t.id
FROM src.video_thumbnails t JOIN stage.video_thumbnail s ON s."ID" = t.id::text
WHERE s.thumbnail_index IS DISTINCT FROM t.thumbnail_index
   OR s.timestamp_seconds IS DISTINCT FROM t.timestamp_seconds
   OR s.created_at IS DISTINCT FROM t.created_at OR s.video_id IS DISTINCT FROM t.video_id::text;

SELECT 'field_mismatch_video_job', j.id
FROM src.video_jobs j JOIN stage.video_job s ON s."ID" = j.id::text
WHERE s.job_type IS DISTINCT FROM j.job_type OR s.status IS DISTINCT FROM j.status
   OR s.priority IS DISTINCT FROM j.priority OR s.parameters IS DISTINCT FROM j.parameters
   OR s.requested_at IS DISTINCT FROM j.requested_at OR s.started_at IS DISTINCT FROM j.started_at
   OR s.completed_at IS DISTINCT FROM j.completed_at OR s.result IS DISTINCT FROM j.result
   OR s.schema_version IS DISTINCT FROM j.schema_version
   OR s.video_id IS DISTINCT FROM j.video_id::text OR s.agent_id IS DISTINCT FROM j.agent_id::text;

SELECT 'field_mismatch_global_snapshot', p.id
FROM src.global_snapshot_positions p JOIN stage.global_snapshot_position s ON s."ID" = p.id::text
WHERE s.position_index IS DISTINCT FROM p.position_index
   OR s.position_type IS DISTINCT FROM p.position_type
   OR s.value IS DISTINCT FROM p.value OR s.created_at IS DISTINCT FROM p.created_at;

SELECT 'field_mismatch_label_option', o.id
FROM src.label_category_options o JOIN stage.label_category_option s ON s."ID" = o.id::text
WHERE s.value IS DISTINCT FROM o.value OR s.sort_order IS DISTINCT FROM o.sort_order
   OR s.created_at IS DISTINCT FROM o.created_at
   OR s.category_key IS DISTINCT FROM stage.uuid_v5('label_categories', o.category_key);

-- Referenzen im Staging: Video-Aktivitaet auf Videos, Jobs auf Agenten, Optionen auf Kategorien.
SELECT 'activity_without_video', x.kind, x.id
FROM (
    SELECT 'rating' AS kind, "ID" AS id, video_id AS vid FROM stage.video_rating
    UNION ALL SELECT 'view', "ID", video_id FROM stage.video_view
    UNION ALL SELECT 'thumbnail', "ID", video_id FROM stage.video_thumbnail
) x
LEFT JOIN stage.video_set v ON v."ID" = x.vid
WHERE v."ID" IS NULL;

SELECT 'job_without_agent', j."ID"
FROM stage.video_job j LEFT JOIN stage.agent a ON a."ID" = j.agent_id
WHERE a."ID" IS NULL;

SELECT 'job_video_missing', j."ID"
FROM stage.video_job j LEFT JOIN stage.video_set v ON v."ID" = j.video_id
WHERE j.video_id IS NOT NULL AND v."ID" IS NULL;

SELECT 'option_without_category', o."ID"
FROM stage.label_category_option o LEFT JOIN stage.label_category c ON c."ID" = o.category_key
WHERE c."ID" IS NULL;
