-- 007 Scene: Pruefungen gegen die Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

SELECT 'row_count', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.scene) s,
     (SELECT count(*) AS n FROM src.scenes) q
WHERE s.n <> q.n;

SELECT 'missing_in_stage', q.id
FROM src.scenes q
LEFT JOIN stage.scene s ON s.legacy_id = q.id
WHERE s."ID" IS NULL;

SELECT 'field_mismatch', q.id
FROM src.scenes q
JOIN stage.scene s ON s.legacy_id = q.id
WHERE s."ID" IS DISTINCT FROM stage.uuid_v5('scenes', q.id)
   OR s.legacy_id IS DISTINCT FROM q.id
   OR s.name IS DISTINCT FROM q.title
   OR s.description IS DISTINCT FROM q.description
   OR s.video_length IS DISTINCT FROM q.video_length
   OR s.video_1080p_size_bytes IS DISTINCT FROM q.video_1080p_size_bytes
   OR s.video_1080p_download IS DISTINCT FROM q.video_1080p_download
   OR s.video_1080p_view IS DISTINCT FROM q.video_1080p_view
   OR s.video_1080p_format IS DISTINCT FROM q.video_1080p_format
   OR s.brand IS DISTINCT FROM q.brand
   OR s.date_released IS DISTINCT FROM q.date_released
   OR s.video_format IS DISTINCT FROM q.video_format
   OR s.video_view IS DISTINCT FROM q.video_view
   OR s.video_download IS DISTINCT FROM q.video_download
   OR s.video_size_bytes IS DISTINCT FROM q.video_size_bytes
   OR s.downloaded IS DISTINCT FROM q.downloaded;

-- Jede Video-Zeile mit Szene verweist auf eine Szene im Staging.
SELECT 'video_scene_missing', v.id::text, v.scene_id
FROM src.videos v
LEFT JOIN stage.scene s ON s."ID" = stage.uuid_v5('scenes', v.scene_id)
WHERE v.scene_id IS NOT NULL AND s."ID" IS NULL;
