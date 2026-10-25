-- 010 Host: Hosts werden aus den Agents abgeleitet. Schluessel ist der Hostname
-- (kleingeschrieben). ID = UUID v5 aus dem Hostnamen, damit jeder Lauf dieselben
-- IDs erzeugt. Jeder Agent bekommt host_id ueber seinen Hostnamen.
-- Die Plattform gehoert zum Host (je Hostname eindeutig, geprueft in 017_host.sql).

CREATE TABLE stage.host (
    "ID"             text         PRIMARY KEY,
    "IsActiveEntity" boolean      NOT NULL,
    name             varchar(255) NOT NULL,
    hostname         varchar(255) NOT NULL UNIQUE,
    platform         varchar(50)  NOT NULL
);

INSERT INTO stage.host
SELECT stage.uuid_v5('hosts', h.hostname), true, h.hostname, h.hostname, h.platform
FROM (
    SELECT DISTINCT ON (lower(a.hostname))
           lower(a.hostname) AS hostname, a.platform
    FROM src.agents a
    ORDER BY lower(a.hostname), a.platform
) h;

ALTER TABLE stage.agent ADD COLUMN host_id text;

UPDATE stage.agent
SET host_id = stage.uuid_v5('hosts', lower(hostname));

ALTER TABLE stage.agent ALTER COLUMN host_id SET NOT NULL;
