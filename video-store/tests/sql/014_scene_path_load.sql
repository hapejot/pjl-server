-- 014 ScenePath / SceneFile / PathSceneMap Load: Pruefungen des Ziels gegen das Staging.

SELECT 'row_count_scene_path', t.n, s.n
FROM (SELECT count(*) AS n FROM public."ScenePathSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.scene_path) s
WHERE t.n <> s.n;

SELECT 'row_count_scene_file', t.n, s.n
FROM (SELECT count(*) AS n FROM public."SceneFileSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.scene_file) s
WHERE t.n <> s.n;

SELECT 'row_count_path_scene_map', t.n, s.n
FROM (SELECT count(*) AS n FROM public."PathSceneMapSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.path_scene_map) s
WHERE t.n <> s.n;

SELECT 'field_mismatch_scene_file', t."ID"
FROM public."SceneFileSet" t JOIN stage.scene_file s ON s."ID" = t."ID"
WHERE t.size::bigint IS DISTINCT FROM s.size
   OR t.name IS DISTINCT FROM s.name
   OR t."path_ID" IS DISTINCT FROM s.path_id;

-- Dateien im Ziel verweisen auf vorhandene Pfade.
SELECT 'scene_file_path_not_in_target', f."ID"
FROM public."SceneFileSet" f
LEFT JOIN public."ScenePathSet" p ON p."ID" = f."path_ID" AND p."IsActiveEntity"
WHERE p."ID" IS NULL;

-- Zuordnungen im Ziel: Szenen-Referenz wie im Staging (bekannte Luecken ausgenommen).
SELECT 'path_scene_map_scene_unexpected', t."ID", t."scene_ID"
FROM public."PathSceneMapSet" t
LEFT JOIN public."SceneSet" s ON s."ID" = t."scene_ID" AND s."IsActiveEntity"
LEFT JOIN stage.reject r ON r.source_table = 'path_scene_map' AND r.key = t."path_ID"
WHERE t."scene_ID" IS NOT NULL AND s."ID" IS NULL AND r.key IS NULL;
