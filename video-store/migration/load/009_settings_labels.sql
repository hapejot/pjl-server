-- Load Einstellungen und Labels. Lauf nur auf leeren Zielen.
-- Reihenfolge: Kategorien vor Optionen (Fremdschluessel).

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."GlobalThumbnailSettingSet")
       OR EXISTS (SELECT 1 FROM public."GlobalSnapshotPositionSet")
       OR EXISTS (SELECT 1 FROM public."LabelCategorySet")
       OR EXISTS (SELECT 1 FROM public."LabelCategoryOptionSet") THEN
        RAISE EXCEPTION 'Einstellungs-/Label-Sets sind nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

SET LOCAL TIME ZONE 'Europe/Berlin';

INSERT INTO public."GlobalThumbnailSettingSet" (
    "ID", "IsActiveEntity", legacy_id, name, updated_at
)
SELECT s."ID", true, s.legacy_id, s.name, stage.odata_ts(s.updated_at)
FROM stage.global_thumbnail_setting s;

INSERT INTO public."GlobalSnapshotPositionSet" (
    "ID", "IsActiveEntity", name, position_index, position_type, value, created_at
)
SELECT s."ID", true, s.name, s.position_index, s.position_type, s.value::text,
       stage.odata_ts(s.created_at)
FROM stage.global_snapshot_position s;

INSERT INTO public."LabelCategorySet" (
    "ID", "IsActiveEntity", legacy_id, name, sort_order, created_at
)
SELECT s."ID", true, s.legacy_id, s.name, s.sort_order, stage.odata_ts(s.created_at)
FROM stage.label_category s;

INSERT INTO public."LabelCategoryOptionSet" (
    "ID", "IsActiveEntity", name, value, created_at, sort_order, "category_ID"
)
SELECT s."ID", true, s.name, s.value, stage.odata_ts(s.created_at), s.sort_order, s.category_key
FROM stage.label_category_option s;
