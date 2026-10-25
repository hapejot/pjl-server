-- Load Actor: stage -> public."ActorSet", public."ActorSceneMapSet".
-- Lauf nur auf leeren Zielen. Voraussetzung: Szenen sind geladen.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."ActorSet")
       OR EXISTS (SELECT 1 FROM public."ActorSceneMapSet") THEN
        RAISE EXCEPTION 'Actor-Sets sind nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

INSERT INTO public."ActorSet" (
    "ID", "IsActiveEntity", legacy_id, name, bio, birth_place, birthday, gender,
    height, last_scene_release_date, measurements, rank, weight, normalized_name
)
SELECT s."ID", true, s.legacy_id, s.name, s.bio, s.birth_place, s.birthday, s.gender,
       s.height, s.last_scene_release_date, s.measurements, s.rank, s.weight, s.normalized_name
FROM stage.actor s;

INSERT INTO public."ActorSceneMapSet" (
    "ID", "IsActiveEntity", name, "actor_ID", "scene_ID"
)
SELECT s."ID", true, s.name, s.actor_id, s.scene_id
FROM stage.actor_scene_map s;
