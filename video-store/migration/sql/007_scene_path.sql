-- 007 ScenePath / SceneFile / PathSceneMap -> stage.scene_path, stage.scene_file,
-- stage.path_scene_map.
--
-- ID = Quell-ID (scene_paths.id bzw. path_id), legacy_id = Quell-ID.
-- PathSceneMap ist 1:1 zum Pfad (PK = path_id), daher ID = path_id.
-- 634 Zuordnungen zeigen auf scene_id-Werte, die es in scenes nicht gibt. Sie
-- bleiben unveraendert (keine Information verloren) und werden in stage.reject
-- protokolliert.

CREATE TABLE stage.scene_path (
    "ID"             text    PRIMARY KEY,
    "IsActiveEntity" boolean NOT NULL,
    legacy_id        text    NOT NULL,
    name             text
);

INSERT INTO stage.scene_path
SELECT p.id, true, p.id, p.name
FROM src.scene_paths p;

CREATE TABLE stage.scene_file (
    "ID"             text    PRIMARY KEY,
    "IsActiveEntity" boolean NOT NULL,
    legacy_id        text    NOT NULL,
    size             bigint,
    name             text,
    path_id          text
);

INSERT INTO stage.scene_file
SELECT f.id, true, f.id, f.size, f.name, f.path_id
FROM src.scene_files f;

CREATE TABLE stage.path_scene_map (
    "ID"             text    PRIMARY KEY,
    "IsActiveEntity" boolean NOT NULL,
    legacy_id        text    NOT NULL,
    name             text    NOT NULL,
    ignore           boolean,
    path_id          text    NOT NULL UNIQUE,
    scene_id         text
);

INSERT INTO stage.path_scene_map
SELECT m.path_id, true, m.path_id, m.path_id || ' / ' || coalesce(m.scene_id, '-'),
       m.ignore, m.path_id, stage.uuid_v5('scenes', m.scene_id)
FROM src.path_scene_map m;

INSERT INTO stage.reject
SELECT 'path_scene_map', m.path_id,
       'scene_id ' || m.scene_id || ' fehlt in scenes (Wert unveraendert uebernommen)'
FROM src.path_scene_map m
WHERE m.scene_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM src.scenes s WHERE s.id = m.scene_id);
