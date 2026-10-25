-- 012 Actor Load: Pruefungen des Ziels gegen das Staging.

SELECT 'row_count_actor', t.n, s.n
FROM (SELECT count(*) AS n FROM public."ActorSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.actor) s
WHERE t.n <> s.n;

SELECT 'row_count_actor_scene_map', t.n, s.n
FROM (SELECT count(*) AS n FROM public."ActorSceneMapSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.actor_scene_map) s
WHERE t.n <> s.n;

SELECT 'field_mismatch_actor', t."ID"
FROM public."ActorSet" t JOIN stage.actor s ON s."ID" = t."ID"
WHERE t.legacy_id IS DISTINCT FROM s.legacy_id
   OR t.name IS DISTINCT FROM s.name
   OR t.birthday IS DISTINCT FROM s.birthday
   OR t.gender IS DISTINCT FROM s.gender
   OR t.normalized_name IS DISTINCT FROM s.normalized_name;

-- Akteur-Tag-Zuordnungen im Ziel zeigen auf vorhandene Akteure.
SELECT 'actor_tag_map_actor_not_in_target', t."ID", t."actor_ID"
FROM public."ActorTagMapSet" t
LEFT JOIN public."ActorSet" a ON a."ID" = t."actor_ID" AND a."IsActiveEntity"
WHERE a."ID" IS NULL;

SELECT 'actor_scene_actor_not_in_target', t."ID"
FROM public."ActorSceneMapSet" t
LEFT JOIN public."ActorSet" a ON a."ID" = t."actor_ID" AND a."IsActiveEntity"
WHERE a."ID" IS NULL;

SELECT 'actor_scene_scene_not_in_target', t."ID"
FROM public."ActorSceneMapSet" t
LEFT JOIN public."SceneSet" s ON s."ID" = t."scene_ID" AND s."IsActiveEntity"
WHERE s."ID" IS NULL;
