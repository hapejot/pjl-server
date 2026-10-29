-- Load Tags: stage -> public.TagSet, ActorTagSet, VideoTagSet, SceneTagMapSet,
-- ActorTagMapSet (Runtime-Format, siehe 001_videoset.sql).
-- Lauf nur auf leeren Zielen. Voraussetzung: VideoSet ist geladen
-- (Fremdschluessel von VideoTagSet).

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."TagSet")
       OR EXISTS (SELECT 1 FROM public."ActorTagSet")
       OR EXISTS (SELECT 1 FROM public."VideoTagSet")
       OR EXISTS (SELECT 1 FROM public."SceneTagMapSet")
       OR EXISTS (SELECT 1 FROM public."ActorTagMapSet") THEN
        RAISE EXCEPTION 'Tag-Sets sind nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

SET LOCAL TIME ZONE 'Europe/Berlin';

INSERT INTO public."TagSet" (
    "ID", "IsActiveEntity", name, normalized_name, created_at, visible
)
SELECT s."ID", true, s.name, s.normalized_name, stage.odata_ts(s.created_at), s.visible
FROM stage.tag s;

INSERT INTO public."ActorTagSet" (
    "ID", "IsActiveEntity", legacy_id, name, category, category_order, normalized_name
)
SELECT s."ID", true, s.legacy_id, s.name, s.category, s.category_order, s.normalized_name
FROM stage.actor_tag s;

INSERT INTO public."VideoTagSet" (
    "ID", "IsActiveEntity", name, source, external_source, confidence,
    created_at, created_by, "video_ID", "tag_ID"
)
SELECT s."ID", true, s.name, s.source, s.external_source, s.confidence::text,
       stage.odata_ts(s.created_at), s.created_by, s.video_id, s.tag_id
FROM stage.video_tag s;

INSERT INTO public."SceneTagMapSet" (
    "ID", "IsActiveEntity", name, "scene_ID", "tag_ID"
)
SELECT s."ID", true, s.name, s.scene_id, s.tag_id
FROM stage.scene_tag_map s;

INSERT INTO public."ActorTagMapSet" (
    "ID", "IsActiveEntity", name, "actor_ID", "tag_ID"
)
SELECT s."ID", true, s.name, s.actor_id, s.tag_id
FROM stage.actor_tag_map s;
