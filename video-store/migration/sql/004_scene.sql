-- 004 Scene: Quelle src.scenes -> Staging stage.scene.
--
-- ID = UUID v5 aus der Quell-ID (stage.uuid_v5), Quell-ID bleibt als legacy_id.
-- Das Zielmodell fuehrt die Quell-ID zusaetzlich als legacy_id. So passen die bereits geladenen
-- VideoSet.scene_ID-Werte ohne Umschluesselung.
-- name = title (nie leer in der Quelle). Alle Textfelder bleiben Text, weil das
-- Zielmodell sie als Edm.String fuehrt.

CREATE TABLE stage.scene (
    "ID"                       text    PRIMARY KEY,
    "IsActiveEntity"           boolean NOT NULL,
    legacy_id                  text    NOT NULL,
    name                       text    NOT NULL,
    description                text,
    video_length               text,
    video_1080p_size_bytes     text,
    video_1080p_download       text,
    video_1080p_view           text,
    video_1080p_format         text,
    brand                      text,
    date_released              text,
    video_format               text,
    video_view                 text,
    video_download             text,
    video_size_bytes           text,
    downloaded                 boolean NOT NULL
);

INSERT INTO stage.scene
SELECT
    stage.uuid_v5('scenes', s.id), true, s.id, s.title, s.description, s.video_length,
    s.video_1080p_size_bytes, s.video_1080p_download, s.video_1080p_view,
    s.video_1080p_format, s.brand, s.date_released, s.video_format,
    s.video_view, s.video_download, s.video_size_bytes, s.downloaded
FROM src.scenes s;
