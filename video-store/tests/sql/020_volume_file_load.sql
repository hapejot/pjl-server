-- 020 Volume und File Load: Pruefungen des Ziels gegen das Staging.

SELECT 'row_count_volume', t.n, s.n
FROM (SELECT count(*) AS n FROM public."VolumeSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.volume) s
WHERE t.n <> s.n;

SELECT 'row_count_file', t.n, s.n
FROM (SELECT count(*) AS n FROM public."FileSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.file) s
WHERE t.n <> s.n;

SELECT 'file_size_sum', t.x, s.x
FROM (SELECT sum(size) AS x FROM public."FileSet") t,
     (SELECT sum(size) AS x FROM stage.file) s
WHERE t.x IS DISTINCT FROM s.x;

-- Referenzen im Ziel: Datei -> Volume (Kompositions-FK), Volume -> Host, Datei -> Video.
SELECT 'file_volume_not_in_target', f."ID"
FROM public."FileSet" f
LEFT JOIN public."VolumeSet" v ON v."ID" = f."volume_ID" AND v."IsActiveEntity"
WHERE v."ID" IS NULL;

SELECT 'volume_host_not_in_target', v."ID"
FROM public."VolumeSet" v
LEFT JOIN public."HostSet" h ON h."ID" = v."host_ID" AND h."IsActiveEntity"
WHERE h."ID" IS NULL;

SELECT 'file_video_not_in_target', f."ID"
FROM public."FileSet" f
LEFT JOIN public."VideoSet" v ON v."ID" = f."video_ID" AND v."IsActiveEntity"
WHERE f."video_ID" IS NOT NULL AND v."ID" IS NULL;

-- Video-Zeilen tragen keine Dateiangaben mehr.
SELECT 'video_has_no_file_columns', count(*)
FROM information_schema.columns
WHERE table_schema = 'public' AND table_name = 'VideoSet'
  AND column_name IN ('file_path', 'file_size', 'sha256_checksum')
HAVING count(*) > 0;
