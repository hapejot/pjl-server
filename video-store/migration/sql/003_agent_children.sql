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

-- Einstellungen sind Schluessel-Wert-Paare je Agent. Die Quelle hat heute feste
-- Spalten; jede Spalte wird eine Zeile. Der Wert ist JSON-Text (Zahl, Text, auch
-- komplexe JSON-Objekte). ID = UUID v5 aus Agent und Schluessel.
CREATE TABLE stage.agent_setting (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    name             text        NOT NULL,
    "key"            text        NOT NULL,
    "value"          text        NOT NULL,
    updated_at       timestamptz NOT NULL,
    agent_id         text        NOT NULL,
    UNIQUE (agent_id, "key")
);

INSERT INTO stage.agent_setting
SELECT stage.uuid_v5('agent_settings', s.agent_id::text || '|' || kv.key), true,
       kv.key, kv.key, kv.value::text, s.updated_at, s.agent_id::text
FROM src.agent_settings s
CROSS JOIN LATERAL jsonb_each(jsonb_build_object(
    'scan_interval_secs',         s.scan_interval_secs,
    'heartbeat_interval_secs',    s.heartbeat_interval_secs,
    'thumbnail_dir',              s.thumbnail_dir,
    'max_concurrent_jobs',        s.max_concurrent_jobs,
    'job_poll_interval_seconds',  s.job_poll_interval_seconds,
    'max_job_history',            s.max_job_history)) AS kv(key, value);

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
