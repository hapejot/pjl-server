-- 008 Video-Aktivitaet: video_ratings, video_views, video_thumbnails, video_jobs
-- -> stage.video_rating, stage.video_view, stage.video_thumbnail, stage.video_job.
-- ID = Quell-UUID als Text. name = ID (wie bei den uebrigen Kindern).
-- video_jobs.video_id ist nullable (Jobs ohne Video), agent_id ist Pflicht.

CREATE TABLE stage.video_rating (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    name             text        NOT NULL,
    rating           integer     NOT NULL,
    created_at       timestamptz NOT NULL,
    updated_at       timestamptz NOT NULL,
    video_id         text        NOT NULL UNIQUE
);

INSERT INTO stage.video_rating
SELECT r.id::text, true, r.id::text, r.rating, r.created_at, r.updated_at, r.video_id::text
FROM src.video_ratings r;

CREATE TABLE stage.video_view (
    "ID"                       text        PRIMARY KEY,
    "IsActiveEntity"           boolean     NOT NULL,
    name                       text        NOT NULL,
    watched_at                 timestamptz NOT NULL,
    watch_duration_seconds     real,
    completed                  boolean,
    video_id                   text        NOT NULL
);

INSERT INTO stage.video_view
SELECT v.id::text, true, v.id::text, v.watched_at, v.watch_duration_seconds, v.completed, v.video_id::text
FROM src.video_views v;

CREATE TABLE stage.video_thumbnail (
    "ID"                text             PRIMARY KEY,
    "IsActiveEntity"    boolean          NOT NULL,
    name                text             NOT NULL,
    thumbnail_index     integer          NOT NULL,
    timestamp_seconds   double precision NOT NULL,
    created_at          timestamptz      NOT NULL,
    video_id            text             NOT NULL,
    UNIQUE (video_id, thumbnail_index)
);

INSERT INTO stage.video_thumbnail
SELECT t.id::text, true, t.id::text, t.thumbnail_index, t.timestamp_seconds,
       t.created_at, t.video_id::text
FROM src.video_thumbnails t;

CREATE TABLE stage.video_job (
    "ID"               text        PRIMARY KEY,
    "IsActiveEntity"   boolean     NOT NULL,
    name               text        NOT NULL,
    job_type           text        NOT NULL,
    status             text        NOT NULL,
    priority           integer     NOT NULL,
    parameters         text,
    requested_at       timestamptz NOT NULL,
    started_at         timestamptz,
    completed_at       timestamptz,
    result             text,
    schema_version     text,
    video_id           text,
    agent_id           text        NOT NULL
);

INSERT INTO stage.video_job
SELECT j.id::text, true, j.id::text, j.job_type, j.status, j.priority, j.parameters,
       j.requested_at, j.started_at, j.completed_at, j.result, j.schema_version,
       j.video_id::text, j.agent_id::text
FROM src.video_jobs j;
