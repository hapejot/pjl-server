-- 009 Tags: Pruefungen Staging gegen Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

-- Zeilenzahlen. Bei Mapping-Tabellen: Quelle minus protokollierte Dubletten.
SELECT 'row_count_tag', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.tag) s, (SELECT count(*) AS n FROM src.tags) q
WHERE s.n <> q.n;

SELECT 'row_count_actor_tag', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.actor_tag) s, (SELECT count(*) AS n FROM src.actor_tags) q
WHERE s.n <> q.n;

SELECT 'row_count_video_tag', s.n, q.n
FROM (SELECT count(*) AS n FROM stage.video_tag) s, (SELECT count(*) AS n FROM src.video_tags) q
WHERE s.n <> q.n;

SELECT 'row_count_scene_tag_map', s.n, q.n - r.n
FROM (SELECT count(*) AS n FROM stage.scene_tag_map) s,
     (SELECT count(*) AS n FROM src.scene_tag_map) q,
     (SELECT coalesce(sum(count_rows), 0) AS n FROM (
         SELECT count(*) - 1 AS count_rows FROM src.scene_tag_map GROUP BY scene_id, tag_id HAVING count(*) > 1) x) r
WHERE s.n <> q.n - r.n;

SELECT 'row_count_actor_tag_map', s.n, q.n - r.n
FROM (SELECT count(*) AS n FROM stage.actor_tag_map) s,
     (SELECT count(*) AS n FROM src.actor_tag_map) q,
     (SELECT coalesce(sum(count_rows), 0) AS n FROM (
         SELECT count(*) - 1 AS count_rows FROM src.actor_tag_map GROUP BY actor_id, tag_id HAVING count(*) > 1) x) r
WHERE s.n <> q.n - r.n;

-- Feldinhalte Tag und Video-Tag.
SELECT 'field_mismatch_tag', t.id::text
FROM src.tags t JOIN stage.tag s ON s."ID" = t.id::text
WHERE s.name IS DISTINCT FROM t.name
   OR s.normalized_name IS DISTINCT FROM t.normalized_name
   OR s.created_at IS DISTINCT FROM t.created_at
   OR s.visible IS DISTINCT FROM t.visible;

SELECT 'field_mismatch_video_tag', vt.id::text
FROM src.video_tags vt JOIN stage.video_tag s ON s."ID" = vt.id::text
WHERE s.source IS DISTINCT FROM vt.source::text
   OR s.external_source IS DISTINCT FROM vt.external_source
   OR s.confidence IS DISTINCT FROM vt.confidence
   OR s.created_at IS DISTINCT FROM vt.created_at
   OR s.created_by IS DISTINCT FROM vt.created_by
   OR s.video_id IS DISTINCT FROM vt.video_id::text
   OR s.tag_id IS DISTINCT FROM vt.tag_id::text;

-- Jedes Paar aus der Quelle ist im Staging enthalten (keine Paare verloren).
SELECT 'missing_scene_pair', q.scene_id, q.tag_id
FROM (SELECT DISTINCT scene_id, tag_id FROM src.scene_tag_map) q
LEFT JOIN stage.scene_tag_map s ON s.scene_id = stage.uuid_v5('scenes', q.scene_id)
  AND s.tag_id = stage.uuid_v5('actor_tags', q.tag_id)
WHERE s."ID" IS NULL;

SELECT 'missing_actor_pair', q.actor_id, q.tag_id
FROM (SELECT DISTINCT actor_id, tag_id FROM src.actor_tag_map) q
LEFT JOIN stage.actor_tag_map s ON s.actor_id = stage.uuid_v5('actors', q.actor_id)
  AND s.tag_id = stage.uuid_v5('actor_tags', q.tag_id)
WHERE s."ID" IS NULL;

-- Referenzen: jedes Video-Tag verweist auf ein Tag, jede Szenen-Zuordnung auf eine Szene.
SELECT 'video_tag_without_tag', s."ID"
FROM stage.video_tag s
LEFT JOIN stage.tag t ON t."ID" = s.tag_id
WHERE t."ID" IS NULL;

SELECT 'scene_tag_without_scene', s."ID"
FROM stage.scene_tag_map s
LEFT JOIN stage.scene sc ON sc."ID" = s.scene_id
WHERE sc."ID" IS NULL;

-- Dubletten-Protokoll: nur Zeilen, die tatsaechlich uebernommen wurden, duerfen
-- im Protokoll fehlen; hier wird geprueft, dass jede Dublette protokolliert ist.
SELECT 'undocumented_duplicate', m.scene_id, m.tag_id
FROM (SELECT scene_id, tag_id FROM src.scene_tag_map GROUP BY 1, 2 HAVING count(*) > 1) m
LEFT JOIN stage.reject r ON r.source_table = 'scene_tag_map' AND r.key = m.scene_id || '|' || m.tag_id
WHERE r.key IS NULL;
