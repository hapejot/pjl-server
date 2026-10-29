-- Load Scene: stage.scene -> public."SceneSet". Lauf nur auf leerem Ziel.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."SceneSet") THEN
        RAISE EXCEPTION 'public.SceneSet ist nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

INSERT INTO public."SceneSet" (
    "ID", "IsActiveEntity", legacy_id, name, description, video_length,
    video_1080p_size_bytes, video_1080p_download, video_1080p_view,
    video_1080p_format, brand, date_released, video_format, video_view,
    video_download, video_size_bytes, downloaded
)
SELECT
    s."ID", true, s.legacy_id, s.name, s.description, s.video_length,
    s.video_1080p_size_bytes, s.video_1080p_download, s.video_1080p_view,
    s.video_1080p_format, s.brand, s.date_released, s.video_format, s.video_view,
    s.video_download, s.video_size_bytes, s.downloaded
FROM stage.scene s;
