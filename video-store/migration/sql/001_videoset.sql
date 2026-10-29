-- 001 VideoSet: Quelle src.videos -> Staging stage.video_set.
--
-- Zielform entspricht der Entitaet Video im Zielmodell:
--   * ID = Quell-UUID als Text (unveraendert, Rueckverfolgung ueber die ID)
--   * IsActiveEntity = true (Fiori-Draft: aktive Instanz)
--   * Datums- und Zahlenfelder mit echten Typen (Quelle: timestamptz, double)
--   * name = file_name (Anzeigename wie bisher)
--   * agent_id / scene_id bleiben vorerst Schluessel ohne Relation; die
--     Relationen kommen mit den Entitaeten Agent und Scene.
-- Keine Zeile wird verworfen: Soft-Delete (deleted_at) bleibt erhalten.

CREATE TABLE stage.video_set (
    "ID"                       text        PRIMARY KEY,
    "IsActiveEntity"           boolean     NOT NULL,
    file_path                  text        NOT NULL,
    name                       varchar(512) NOT NULL,
    file_size                  bigint      NOT NULL,
    duration_seconds           double precision,
    width                      integer,
    height                     integer,
    codec                      varchar(50),
    bitrate                    bigint,
    is_streamable              boolean,
    thumbnail_path             text,
    created_at                 timestamptz NOT NULL,
    updated_at                 timestamptz NOT NULL,
    last_seen                  timestamptz NOT NULL,
    thumbnail_count            integer,
    deleted_at                 timestamptz,
    pending_physical_delete    boolean,
    title                      varchar(512),
    is_reviewed                boolean     NOT NULL,
    sha256_checksum            varchar(64),
    agent_id                   text        NOT NULL,
    scene_id                   text
);

INSERT INTO stage.video_set (
    "ID", "IsActiveEntity", file_path, name, file_size, duration_seconds,
    width, height, codec, bitrate, is_streamable, thumbnail_path,
    created_at, updated_at, last_seen, thumbnail_count, deleted_at,
    pending_physical_delete, title, is_reviewed, sha256_checksum,
    agent_id, scene_id
)
SELECT
    v.id::text, true, v.file_path, v.file_name, v.file_size, v.duration_seconds,
    v.width, v.height, v.codec, v.bitrate, v.is_streamable, v.thumbnail_path,
    v.created_at, v.updated_at, v.last_seen, v.thumbnail_count, v.deleted_at,
    v.pending_physical_delete, v.title, v.is_reviewed, v.sha256_checksum,
    v.agent_id::text, stage.uuid_v5('scenes', v.scene_id)
FROM src.videos v;
