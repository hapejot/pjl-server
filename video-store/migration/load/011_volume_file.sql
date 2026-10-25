-- Load Volume und File: stage -> public."VolumeSet", public."FileSet".
-- Lauf nur auf leeren Zielen. Volume nach Host (Kompositions-FK), File nach Volume und Video.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM public."VolumeSet") OR EXISTS (SELECT 1 FROM public."FileSet") THEN
        RAISE EXCEPTION 'VolumeSet/FileSet sind nicht leer - Load nur auf leerem Ziel';
    END IF;
END
$$;

INSERT INTO public."VolumeSet" ("ID", "IsActiveEntity", name, mount_path, "host_ID")
SELECT s."ID", true, s.name, s.mount_path, s.host_id
FROM stage.volume s;

INSERT INTO public."FileSet" (
    "ID", "IsActiveEntity", name, relative_path, size, sha256_checksum, "volume_ID", "video_ID"
)
SELECT s."ID", true, s.name, s.relative_path, s.size, s.sha256_checksum, s.volume_id, s.video_id
FROM stage.file s;
