-- 013 ScenePath / SceneFile / PathSceneMap: Pruefungen Staging gegen Quelle.
-- Jede Abfrage gibt nur bei Fehlern Zeilen aus.

SELECT 'row_count_scene_path', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.scene_path) s, (SELECT count(*) AS n FROM src.scene_paths) q
WHERE s.n <> q.n;

SELECT 'row_count_scene_file', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.scene_file) s, (SELECT count(*) AS n FROM src.scene_files) q
WHERE s.n <> q.n;

SELECT 'row_count_path_scene_map', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.path_scene_map) s, (SELECT count(*) AS n FROM src.path_scene_map) q
WHERE s.n <> q.n;

SELECT 'field_mismatch_scene_path', p.id
FROM src.scene_paths p JOIN stage.scene_path s ON s."ID" = p.id
WHERE s.name IS DISTINCT FROM p.name;

SELECT 'field_mismatch_scene_file', f.id
FROM src.scene_files f JOIN stage.scene_file s ON s."ID" = f.id
WHERE s.size IS DISTINCT FROM f.size
   OR s.name IS DISTINCT FROM f.name
   OR s.path_id IS DISTINCT FROM f.path_id;

SELECT 'field_mismatch_path_scene_map', m.path_id
FROM src.path_scene_map m JOIN stage.path_scene_map s ON s.path_id = m.path_id
WHERE s.scene_id IS DISTINCT FROM stage.uuid_v5('scenes', m.scene_id)
   OR s.ignore IS DISTINCT FROM m.ignore;

-- Referenzen im Staging: Datei -> Pfad, Pfad -> Zuordnung.
SELECT 'scene_file_without_path', f."ID"
FROM stage.scene_file f
LEFT JOIN stage.scene_path p ON p."ID" = f.path_id
WHERE p."ID" IS NULL;

SELECT 'path_without_map', p."ID"
FROM stage.scene_path p
LEFT JOIN stage.path_scene_map m ON m.path_id = p."ID"
WHERE m.path_id IS NULL;

-- Fehlende Szenen sind genau die protokollierten Faelle.
SELECT 'unlogged_missing_scene', m.path_id, m.scene_id
FROM stage.path_scene_map m
LEFT JOIN stage.scene s ON s."ID" = m.scene_id
LEFT JOIN stage.reject r ON r.source_table = 'path_scene_map' AND r.key = m.path_id
WHERE m.scene_id IS NOT NULL AND s."ID" IS NULL AND r.key IS NULL;
