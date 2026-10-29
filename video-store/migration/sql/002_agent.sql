-- 002 Agent: Quelle src.agents -> Staging stage.agent.
-- ID = Quell-UUID als Text (wie bei VideoSet, damit agent_ID passt).
-- status wird als Text uebernommen (Quelle: Enum agent_status).

CREATE TABLE stage.agent (
    "ID"             text         PRIMARY KEY,
    "IsActiveEntity" boolean      NOT NULL,
    name             varchar(255) NOT NULL,
    hostname         varchar(255) NOT NULL,
    base_url         varchar(512) NOT NULL,
    platform         varchar(50)  NOT NULL,
    status           text         NOT NULL,
    last_heartbeat   timestamptz  NOT NULL,
    created_at       timestamptz  NOT NULL,
    version          varchar(255)
);

INSERT INTO stage.agent (
    "ID", "IsActiveEntity", name, hostname, base_url, platform,
    status, last_heartbeat, created_at, version
)
SELECT
    a.id::text, true, a.name, a.hostname, a.base_url, a.platform,
    a.status::text, a.last_heartbeat, a.created_at, a.version
FROM src.agents a;
