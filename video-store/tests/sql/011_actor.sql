-- 011 Actor: Pruefungen Staging gegen Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

SELECT 'row_count_actor', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.actor) s, (SELECT count(*) AS n FROM src.actors) q
WHERE s.n <> q.n;

SELECT 'row_count_actor_scene_map', s.n, q.n - r.n
FROM (SELECT count(*) AS n FROM stage.actor_scene_map) s,
     (SELECT count(*) AS n FROM src.actor_scene_map) q,
     (SELECT coalesce(sum(c - 1), 0) AS n FROM (
         SELECT count(*) AS c FROM src.actor_scene_map GROUP BY actor_id, scene_id HAVING count(*) > 1) x) r
WHERE s.n <> q.n - r.n;

SELECT 'field_mismatch_actor', a.id
FROM src.actors a JOIN stage.actor s ON s.legacy_id = a.id
WHERE s."ID" IS DISTINCT FROM stage.uuid_v5('actors', a.id)
   OR s.name IS DISTINCT FROM a.name
   OR s.bio IS DISTINCT FROM a.bio
   OR s.birth_place IS DISTINCT FROM a.birth_place
   OR s.birthday IS DISTINCT FROM a.birthday
   OR s.gender IS DISTINCT FROM a.gender
   OR s.height IS DISTINCT FROM a.height
   OR s.last_scene_release_date IS DISTINCT FROM a.last_scene_release_date
   OR s.measurements IS DISTINCT FROM a.measurements
   OR s.rank IS DISTINCT FROM a.rank
   OR s.weight IS DISTINCT FROM a.weight
   OR s.normalized_name IS DISTINCT FROM a.normalized_name;

-- Jedes Paar aus der Quelle ist im Staging enthalten.
SELECT 'missing_actor_scene_pair', q.actor_id, q.scene_id
FROM (SELECT DISTINCT actor_id, scene_id FROM src.actor_scene_map) q
LEFT JOIN stage.actor_scene_map s ON s.actor_id = stage.uuid_v5('actors', q.actor_id)
  AND s.scene_id = stage.uuid_v5('scenes', q.scene_id)
WHERE s."ID" IS NULL;

-- Referenzen: jede Zuordnung verweist auf einen Akteur und eine Szene im Staging.
SELECT 'actor_scene_without_actor', s."ID"
FROM stage.actor_scene_map s
LEFT JOIN stage.actor a ON a."ID" = s.actor_id
WHERE a."ID" IS NULL;

SELECT 'actor_scene_without_scene', s."ID"
FROM stage.actor_scene_map s
LEFT JOIN stage.scene sc ON sc."ID" = s.scene_id
WHERE sc."ID" IS NULL;
