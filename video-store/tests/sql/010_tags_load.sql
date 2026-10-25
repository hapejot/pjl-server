-- 010 Tags Load: Pruefungen des Ziels gegen das Staging.

SELECT 'row_count_tag', t.n, s.n
FROM (SELECT count(*) AS n FROM public."TagSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.tag) s
WHERE t.n <> s.n;

SELECT 'row_count_actor_tag', t.n, s.n
FROM (SELECT count(*) AS n FROM public."ActorTagSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.actor_tag) s
WHERE t.n <> s.n;

SELECT 'row_count_video_tag', t.n, s.n
FROM (SELECT count(*) AS n FROM public."VideoTagSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.video_tag) s
WHERE t.n <> s.n;

SELECT 'row_count_scene_tag_map', t.n, s.n
FROM (SELECT count(*) AS n FROM public."SceneTagMapSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.scene_tag_map) s
WHERE t.n <> s.n;

SELECT 'row_count_actor_tag_map', t.n, s.n
FROM (SELECT count(*) AS n FROM public."ActorTagMapSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.actor_tag_map) s
WHERE t.n <> s.n;

-- Zeitstempel im Runtime-Format und Zeitpunkt erhalten.
SELECT 'timestamp_value_tag', t."ID"
FROM public."TagSet" t
JOIN stage.tag s ON s."ID" = t."ID"
WHERE t.created_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.created_at)
   OR t.created_at !~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}(Z|[+-]\d{2}:\d{2})$';

SELECT 'timestamp_value_video_tag', t."ID"
FROM public."VideoTagSet" t
JOIN stage.video_tag s ON s."ID" = t."ID"
WHERE t.created_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.created_at);

-- Konfidenz: gleicher Zahlenwert wie im Staging.
SELECT 'confidence_value', t."ID"
FROM public."VideoTagSet" t
JOIN stage.video_tag s ON s."ID" = t."ID"
WHERE t.confidence::real IS DISTINCT FROM s.confidence;

-- Referenzen im Ziel: Video-Tags auf Video und Tag, Szenen- und Akteur-Zuordnungen.
SELECT 'video_tag_video_not_in_target', t."ID"
FROM public."VideoTagSet" t
LEFT JOIN public."VideoSet" v ON v."ID" = t."video_ID" AND v."IsActiveEntity"
WHERE v."ID" IS NULL;

SELECT 'video_tag_tag_not_in_target', t."ID"
FROM public."VideoTagSet" t
LEFT JOIN public."TagSet" g ON g."ID" = t."tag_ID" AND g."IsActiveEntity"
WHERE g."ID" IS NULL;

SELECT 'scene_tag_tag_not_in_target', t."ID"
FROM public."SceneTagMapSet" t
LEFT JOIN public."ActorTagSet" g ON g."ID" = t."tag_ID" AND g."IsActiveEntity"
WHERE g."ID" IS NULL;

SELECT 'scene_tag_scene_not_in_target', t."ID"
FROM public."SceneTagMapSet" t
LEFT JOIN public."SceneSet" s ON s."ID" = t."scene_ID" AND s."IsActiveEntity"
WHERE s."ID" IS NULL;

-- Eindeutigkeit der Paare im Ziel.
SELECT 'dup_scene_tag_pair', t."scene_ID", t."tag_ID"
FROM public."SceneTagMapSet" t
GROUP BY t."scene_ID", t."tag_ID"
HAVING count(*) > 1;
