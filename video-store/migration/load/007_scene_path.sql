-- Load ScenePath, SceneFile, PathSceneMap. Lauf nur auf leeren Zielen.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."ScenePathSet")
       OR EXISTS (SELECT 1 FROM public."SceneFileSet")
       OR EXISTS (SELECT 1 FROM public."PathSceneMapSet") THEN
        RAISE EXCEPTION 'Scene-Pfad-Sets sind nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

INSERT INTO public."ScenePathSet" ("ID", "IsActiveEntity", legacy_id, name)
SELECT s."ID", true, s.legacy_id, s.name
FROM stage.scene_path s;

INSERT INTO public."SceneFileSet" ("ID", "IsActiveEntity", legacy_id, size, name, "path_ID")
SELECT s."ID", true, s.legacy_id, s.size, s.name, s.path_id
FROM stage.scene_file s;

INSERT INTO public."PathSceneMapSet" ("ID", "IsActiveEntity", legacy_id, name, ignore, "path_ID", "scene_ID")
SELECT s."ID", true, s.legacy_id, s.name, s.ignore, s.path_id, s.scene_id
FROM stage.path_scene_map s;
