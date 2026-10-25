-- 003 Agent-Kinder: agent_capabilities, agent_settings, agent_video_directories
-- -> stage.agent_capability, stage.agent_setting, stage.agent_video_directory.
--
-- ID = Quell-UUID als Text. name = ID (wie im bisherigen Store, dort aus dem
-- Schluessel abgeleitet). agent_id bleibt Schluessel; der Load setzt daraus
-- die Referenz agent_ID.

CREATE TABLE stage.agent_capability (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    name             text        NOT NULL,
    job_type         text        NOT NULL,
    schema_version   text        NOT NULL,
    parameter_schema jsonb       NOT NULL,
    result_schema    jsonb       NOT NULL,
    requires_video   boolean     NOT NULL,
    registered_at    timestamptz NOT NULL,
    updated_at       timestamptz NOT NULL,
    agent_id         text        NOT NULL,
    UNIQUE (agent_id, job_type)
);

INSERT INTO stage.agent_capability
SELECT c.id::text, true, c.id::text, c.job_type, c.schema_version,
       c.parameter_schema, c.result_schema, c.requires_video,
       c.registered_at, c.updated_at, c.agent_id::text
FROM src.agent_capabilities c;

CREATE TABLE stage.agent_setting (
    "ID"                       text        PRIMARY KEY,
    "IsActiveEntity"           boolean     NOT NULL,
    name                       text        NOT NULL,
    scan_interval_secs         integer     NOT NULL,
    heartbeat_interval_secs    integer     NOT NULL,
    updated_at                 timestamptz NOT NULL,
    thumbnail_dir              text        NOT NULL,
    max_concurrent_jobs        integer     NOT NULL,
    job_poll_interval_seconds  integer     NOT NULL,
    max_job_history            integer     NOT NULL,
    agent_id                   text        NOT NULL UNIQUE
);

INSERT INTO stage.agent_setting
SELECT s.id::text, true, s.id::text, s.scan_interval_secs, s.heartbeat_interval_secs,
       s.updated_at, s.thumbnail_dir, s.max_concurrent_jobs,
       s.job_poll_interval_seconds, s.max_job_history, s.agent_id::text
FROM src.agent_settings s;

CREATE TABLE stage.agent_video_directory (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    name             text        NOT NULL,
    path             text        NOT NULL,
    recursive        boolean     NOT NULL,
    created_at       timestamptz NOT NULL,
    agent_id         text        NOT NULL,
    UNIQUE (agent_id, path)
);

INSERT INTO stage.agent_video_directory
SELECT d.id::text, true, d.id::text, d.path, d.recursive, d.created_at, d.agent_id::text
FROM src.agent_video_directories d;
