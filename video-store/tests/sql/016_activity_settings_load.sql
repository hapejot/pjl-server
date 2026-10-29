-- 016 Load Video-Aktivitaet und Einstellungen: Pruefungen des Ziels gegen das Staging.

SELECT 'row_count_video_rating', t.n, s.n
FROM (SELECT count(*) AS n FROM public."VideoRatingSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.video_rating) s WHERE t.n <> s.n;

SELECT 'row_count_video_view', t.n, s.n
FROM (SELECT count(*) AS n FROM public."VideoViewSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.video_view) s WHERE t.n <> s.n;

SELECT 'row_count_video_thumbnail', t.n, s.n
FROM (SELECT count(*) AS n FROM public."VideoThumbnailSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.video_thumbnail) s WHERE t.n <> s.n;

SELECT 'row_count_video_job', t.n, s.n
FROM (SELECT count(*) AS n FROM public."VideoJobSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.video_job) s WHERE t.n <> s.n;

SELECT 'row_count_global_thumbnail', t.n, s.n
FROM (SELECT count(*) AS n FROM public."GlobalThumbnailSettingSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.global_thumbnail_setting) s WHERE t.n <> s.n;

SELECT 'row_count_global_snapshot', t.n, s.n
FROM (SELECT count(*) AS n FROM public."GlobalSnapshotPositionSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.global_snapshot_position) s WHERE t.n <> s.n;

SELECT 'row_count_label_category', t.n, s.n
FROM (SELECT count(*) AS n FROM public."LabelCategorySet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.label_category) s WHERE t.n <> s.n;

SELECT 'row_count_label_option', t.n, s.n
FROM (SELECT count(*) AS n FROM public."LabelCategoryOptionSet" WHERE "IsActiveEntity") t,
     (SELECT count(*) AS n FROM stage.label_category_option) s WHERE t.n <> s.n;

-- Zeitstempel im Runtime-Format, Zeitpunkt erhalten.
SELECT 'timestamp_value_video_job', t."ID"
FROM public."VideoJobSet" t JOIN stage.video_job s ON s."ID" = t."ID"
WHERE t.requested_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.requested_at)
   OR t.started_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.started_at)
   OR t.completed_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.completed_at);

SELECT 'timestamp_value_rating', t."ID"
FROM public."VideoRatingSet" t JOIN stage.video_rating s ON s."ID" = t."ID"
WHERE t.created_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.created_at)
   OR t.updated_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.updated_at);

SELECT 'timestamp_value_view', t."ID"
FROM public."VideoViewSet" t JOIN stage.video_view s ON s."ID" = t."ID"
WHERE t.watched_at::timestamptz IS DISTINCT FROM date_trunc('milliseconds', s.watched_at);

-- Dauer und Zeitoffset als Text: gleicher Zahlenwert wie im Staging.
SELECT 'double_value_thumbnail', t."ID"
FROM public."VideoThumbnailSet" t JOIN stage.video_thumbnail s ON s."ID" = t."ID"
WHERE t.timestamp_seconds::double precision IS DISTINCT FROM s.timestamp_seconds;

SELECT 'double_value_view', t."ID"
FROM public."VideoViewSet" t JOIN stage.video_view s ON s."ID" = t."ID"
WHERE t.watch_duration_seconds::real IS DISTINCT FROM s.watch_duration_seconds;

SELECT 'double_value_snapshot', t."ID"
FROM public."GlobalSnapshotPositionSet" t JOIN stage.global_snapshot_position s ON s."ID" = t."ID"
WHERE t.value::double precision IS DISTINCT FROM s.value;

-- Referenzen im Ziel.
SELECT 'activity_video_not_in_target', x.kind, x.id
FROM (
    SELECT 'rating' AS kind, "ID" AS id, "video_ID" AS vid FROM public."VideoRatingSet" WHERE "IsActiveEntity"
    UNION ALL SELECT 'view', "ID", "video_ID" FROM public."VideoViewSet" WHERE "IsActiveEntity"
    UNION ALL SELECT 'thumbnail', "ID", "video_ID" FROM public."VideoThumbnailSet" WHERE "IsActiveEntity"
) x
LEFT JOIN public."VideoSet" v ON v."ID" = x.vid AND v."IsActiveEntity"
WHERE v."ID" IS NULL;

SELECT 'job_agent_not_in_target', j."ID"
FROM public."VideoJobSet" j
LEFT JOIN public."AgentSet" a ON a."ID" = j."agent_ID" AND a."IsActiveEntity"
WHERE a."ID" IS NULL;

SELECT 'option_category_not_in_target', o."ID"
FROM public."LabelCategoryOptionSet" o
LEFT JOIN public."LabelCategorySet" c ON c."ID" = o."category_ID" AND c."IsActiveEntity"
WHERE c."ID" IS NULL;
