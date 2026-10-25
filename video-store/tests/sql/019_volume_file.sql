-- 019 Volume und File: Pruefungen Staging gegen Quelle. Jede Abfrage gibt nur bei Fehlern Zeilen aus.

-- Pfad-Rekonstruktion: Volume-Mount plus relativer Pfad ergibt wieder den Quellpfad.
CREATE TEMP VIEW tmp_file_path AS
SELECT f."ID" AS file_id,
       v.mount_path || CASE WHEN right(v.mount_path, 2) = ':\'
                            THEN replace(f.relative_path, '/', '\')
                            ELSE '/' || f.relative_path END AS full_path
FROM stage.file f JOIN stage.volume v ON v."ID" = f.volume_id;

-- Jedes Video hat genau eine Datei, und deren Pfad ist der Quellpfad.
SELECT 'video_file_count', v.id::text, count(f."ID")
FROM src.videos v
LEFT JOIN stage.file f ON f.video_id = v.id::text
GROUP BY v.id
HAVING count(f."ID") <> 1;

SELECT 'video_path_roundtrip', v.id::text, r.full_path, v.file_path
FROM src.videos v
JOIN stage.file f ON f.video_id = v.id::text
JOIN tmp_file_path r ON r.file_id = f."ID"
WHERE r.full_path IS DISTINCT FROM v.file_path;

-- Jeder Scene-Dateipfad ist als Datei vorhanden (auch dort, wo er mit einem Video zusammenfaellt).
SELECT 'scene_path_missing', sp.name || '/' || sf.name
FROM src.scene_files sf
JOIN src.scene_paths sp ON sp.id = sf.path_id
WHERE NOT EXISTS (SELECT 1 FROM tmp_file_path r WHERE r.full_path = sp.name || '/' || sf.name);

-- Groessen: Videos stimmen, Dateien haben keine Groesse verloren.
SELECT 'video_size_sum', s.x, q.x
FROM (SELECT sum(f.size) AS x FROM stage.file f WHERE f.video_id IS NOT NULL) s,
     (SELECT sum(file_size) AS x FROM src.videos) q
WHERE s.x IS DISTINCT FROM q.x;

-- Relative Pfade sind Schraegstrich-getrennt, nie leer, nie absolut.
SELECT 'bad_relative_path', f."ID", f.relative_path
FROM stage.file f
WHERE f.relative_path LIKE '%\%' OR f.relative_path = '' OR f.relative_path LIKE '/%';

-- Jedes Volume hat genau einen Host, und jede Datei genau ein Volume.
SELECT 'file_without_volume', f."ID"
FROM stage.file f LEFT JOIN stage.volume v ON v."ID" = f.volume_id
WHERE v."ID" IS NULL;

-- Die protokollierten Zusammenfuehrungen entsprechen den Quelldubletten.
SELECT 'reject_count', r.n, q.n
FROM (SELECT count(*) AS n FROM stage.reject WHERE source_table = 'file') r,
     (SELECT count(*) AS n FROM (
         SELECT 1 FROM stage.file_candidate GROUP BY host_id, mount_path, relative_path HAVING count(*) > 1) x) q
WHERE r.n <> q.n;
