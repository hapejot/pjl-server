-- 006 Actor: actors -> stage.actor, actor_scene_map -> stage.actor_scene_map.
-- Actor: ID = Quell-ID, legacy_id = Quell-ID (wie bei Scene und ActorTag).
-- Name ist nicht eindeutig (7750 Akteure, 7316 Namen), daher bleibt die ID der Schluessel.
-- ActorSceneMap ohne PK in der Quelle: ID deterministisch aus dem Paar (MD5 als UUID).

CREATE TABLE stage.actor (
    "ID"                       text    PRIMARY KEY,
    "IsActiveEntity"           boolean NOT NULL,
    legacy_id                  text    NOT NULL,
    name                       text,
    bio                        text,
    birth_place                text,
    birthday                   text,
    gender                     text,
    height                     text,
    last_scene_release_date    text,
    measurements               text,
    rank                       text,
    weight                     text,
    normalized_name            text    NOT NULL
);

INSERT INTO stage.actor
SELECT stage.uuid_v5('actors', a.id), true, a.id, a.name, a.bio, a.birth_place, a.birthday, a.gender, a.height,
       a.last_scene_release_date, a.measurements, a.rank, a.weight, a.normalized_name
FROM src.actors a;

CREATE TABLE stage.actor_scene_map (
    "ID"             text    PRIMARY KEY,
    "IsActiveEntity" boolean NOT NULL,
    name             text    NOT NULL,
    actor_id         text    NOT NULL,
    scene_id         text    NOT NULL,
    UNIQUE (actor_id, scene_id)
);

-- Exakte Dubletten (aktuell keine) wuerden hier protokolliert.
INSERT INTO stage.reject
SELECT 'actor_scene_map', m.actor_id || '|' || m.scene_id,
       'exakte Dublette (' || count(*) || ' Zeilen, 1 uebernommen)'
FROM src.actor_scene_map m
GROUP BY m.actor_id, m.scene_id
HAVING count(*) > 1;

INSERT INTO stage.actor_scene_map
SELECT DISTINCT
    md5(m.actor_id || '|' || m.scene_id)::uuid::text, true,
    m.actor_id || ' / ' || m.scene_id,
    stage.uuid_v5('actors', m.actor_id), stage.uuid_v5('scenes', m.scene_id)
FROM src.actor_scene_map m;
