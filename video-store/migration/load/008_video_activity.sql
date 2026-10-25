-- Load Video-Aktivitaet: stage -> public.VideoRatingSet, VideoViewSet, VideoThumbnailSet,
-- VideoJobSet. Lauf nur auf leeren Zielen. Voraussetzung: VideoSet und AgentSet sind geladen.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."VideoRatingSet")
       OR EXISTS (SELECT 1 FROM public."VideoViewSet")
       OR EXISTS (SELECT 1 FROM public."VideoThumbnailSet")
       OR EXISTS (SELECT 1 FROM public."VideoJobSet") THEN
        RAISE EXCEPTION 'Video-Aktivitaets-Sets sind nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

SET LOCAL TIME ZONE 'Europe/Berlin';

INSERT INTO public."VideoRatingSet" (
    "ID", "IsActiveEntity", name, rating, created_at, updated_at, "video_ID"
)
SELECT s."ID", true, s.name, s.rating, stage.odata_ts(s.created_at),
       stage.odata_ts(s.updated_at), s.video_id
FROM stage.video_rating s;

INSERT INTO public."VideoViewSet" (
    "ID", "IsActiveEntity", name, watched_at, watch_duration_seconds, completed, "video_ID"
)
SELECT s."ID", true, s.name, stage.odata_ts(s.watched_at),
       s.watch_duration_seconds::text, s.completed, s.video_id
FROM stage.video_view s;

INSERT INTO public."VideoThumbnailSet" (
    "ID", "IsActiveEntity", name, thumbnail_index, timestamp_seconds, created_at, "video_ID"
)
SELECT s."ID", true, s.name, s.thumbnail_index, s.timestamp_seconds::text,
       stage.odata_ts(s.created_at), s.video_id
FROM stage.video_thumbnail s;

INSERT INTO public."VideoJobSet" (
    "ID", "IsActiveEntity", name, job_type, status, priority, parameters,
    requested_at, started_at, completed_at, result, schema_version, "video_ID", "agent_ID"
)
SELECT s."ID", true, s.name, s.job_type, s.status, s.priority, s.parameters,
       stage.odata_ts(s.requested_at), stage.odata_ts(s.started_at),
       stage.odata_ts(s.completed_at), s.result, s.schema_version, s.video_id, s.agent_id
FROM stage.video_job s;
