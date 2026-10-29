-- 008 Scene Load: Pruefungen des Ziels gegen das Staging.

SELECT 'row_count', t.n, s.n
FROM (SELECT count(*) AS n FROM public."SceneSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.scene) s
WHERE t.n <> s.n;

SELECT 'missing_in_target', s."ID"
FROM stage.scene s
LEFT JOIN public."SceneSet" t ON t."ID" = s."ID" AND t."IsActiveEntity"
WHERE t."ID" IS NULL;

SELECT 'field_mismatch', t."ID"
FROM public."SceneSet" t
JOIN stage.scene s ON s."ID" = t."ID"
WHERE t.legacy_id IS DISTINCT FROM s.legacy_id
   OR t.name IS DISTINCT FROM s.name
   OR t.description IS DISTINCT FROM s.description
   OR t.video_length IS DISTINCT FROM s.video_length
   OR t.video_1080p_size_bytes IS DISTINCT FROM s.video_1080p_size_bytes
   OR t.brand IS DISTINCT FROM s.brand
   OR t.date_released IS DISTINCT FROM s.date_released
   OR t.video_size_bytes IS DISTINCT FROM s.video_size_bytes
   OR t.downloaded IS DISTINCT FROM s.downloaded;

-- Videos im Ziel verweisen nur auf vorhandene Szenen.
SELECT 'video_scene_not_in_target', v."ID", v."scene_ID"
FROM public."VideoSet" v
LEFT JOIN public."SceneSet" s ON s."ID" = v."scene_ID" AND s."IsActiveEntity"
WHERE v."scene_ID" IS NOT NULL AND s."ID" IS NULL;
