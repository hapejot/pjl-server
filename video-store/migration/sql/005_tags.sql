-- 005 Tags: Quelle -> Staging.
--
--   tags              -> stage.tag            (Tag)
--   actor_tags        -> stage.actor_tag      (ActorTag, ID = Quell-ID, legacy_id = Quell-ID)
--   video_tags        -> stage.video_tag      (VideoTag, name = source wie im bisherigen Store)
--   scene_tag_map     -> stage.scene_tag_map  (SceneTagMap, ohne PK in der Quelle)
--   actor_tag_map     -> stage.actor_tag_map  (ActorTagMap, ohne PK in der Quelle)
--
-- Mapping-Tabellen haben keinen Primaerschluessel. Die ID wird deterministisch
-- aus dem Paar gebildet (MD5 als UUID). Exakte Dubletten werden entfernt und
-- in stage.reject protokolliert.

CREATE TABLE stage.tag (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    name             varchar(100) NOT NULL,
    normalized_name  varchar(100) NOT NULL UNIQUE,
    created_at       timestamptz NOT NULL,
    visible          boolean     NOT NULL
);

INSERT INTO stage.tag
SELECT t.id::text, true, t.name, t.normalized_name, t.created_at, t.visible
FROM src.tags t;

CREATE TABLE stage.actor_tag (
    "ID"             text  PRIMARY KEY,
    "IsActiveEntity" boolean NOT NULL,
    legacy_id        text  NOT NULL,
    name             text,
    category         text,
    category_order   text,
    normalized_name  text  NOT NULL
);

INSERT INTO stage.actor_tag
SELECT stage.uuid_v5('actor_tags', t.id), true, t.id, t.name, t.category, t.category_order, t.normalized_name
FROM src.actor_tags t;

CREATE TABLE stage.video_tag (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    name             text        NOT NULL,
    source           text        NOT NULL,
    external_source  varchar(100),
    confidence       real,
    created_at       timestamptz NOT NULL,
    created_by       varchar(100),
    video_id         text        NOT NULL,
    tag_id           text        NOT NULL,
    UNIQUE (video_id, tag_id, source)
);

INSERT INTO stage.video_tag
SELECT vt.id::text, true, vt.source::text, vt.source::text, vt.external_source,
       vt.confidence, vt.created_at, vt.created_by, vt.video_id::text, vt.tag_id::text
FROM src.video_tags vt;

CREATE TABLE stage.reject (
    source_table text NOT NULL,
    key          text NOT NULL,
    reason       text NOT NULL
);

CREATE TABLE stage.scene_tag_map (
    "ID"             text    PRIMARY KEY,
    "IsActiveEntity" boolean NOT NULL,
    name             text    NOT NULL,
    scene_id         text    NOT NULL,
    tag_id           text    NOT NULL,
    UNIQUE (scene_id, tag_id)
);

-- Exakte Dubletten: mehrfach vorkommende Paare, Zeile zaehlt nur einmal.
INSERT INTO stage.reject
SELECT 'scene_tag_map', m.scene_id || '|' || m.tag_id,
       'exakte Dublette (' || count(*) || ' Zeilen, 1 uebernommen)'
FROM src.scene_tag_map m
GROUP BY m.scene_id, m.tag_id
HAVING count(*) > 1;

INSERT INTO stage.scene_tag_map
SELECT DISTINCT
    md5(m.scene_id || '|' || m.tag_id)::uuid::text, true,
    m.scene_id || ' / ' || m.tag_id,
    stage.uuid_v5('scenes', m.scene_id), stage.uuid_v5('actor_tags', m.tag_id)
FROM src.scene_tag_map m;

CREATE TABLE stage.actor_tag_map (
    "ID"             text    PRIMARY KEY,
    "IsActiveEntity" boolean NOT NULL,
    name             text    NOT NULL,
    actor_id         text    NOT NULL,
    tag_id           text    NOT NULL,
    UNIQUE (actor_id, tag_id)
);

INSERT INTO stage.reject
SELECT 'actor_tag_map', m.actor_id || '|' || m.tag_id,
       'exakte Dublette (' || count(*) || ' Zeilen, 1 uebernommen)'
FROM src.actor_tag_map m
GROUP BY m.actor_id, m.tag_id
HAVING count(*) > 1;

INSERT INTO stage.actor_tag_map
SELECT DISTINCT
    md5(m.actor_id || '|' || m.tag_id)::uuid::text, true,
    m.actor_id || ' / ' || m.tag_id,
    stage.uuid_v5('actors', m.actor_id), stage.uuid_v5('actor_tags', m.tag_id)
FROM src.actor_tag_map m;
