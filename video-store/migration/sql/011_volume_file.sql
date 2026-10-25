-- 011 Volume und File.
--
-- Quellen fuer Dateien: videos (Host = Host des Agents) und scene_files (Host nuc).
-- Volume = Mount, unter dem die Datei auf dem Host liegt:
--   Windows:  Laufwerksbuchstabe, z. B. F:\
--   Unix:     /mnt/NN/peter, /mnt/ssd-local/peter, /media/NN/peter, /home/peter
-- File wird ueber den relativen Pfad im Volume adressiert (Trennzeichen '/').
--
-- Dubletten (gleiches Volume und gleicher relativer Pfad) werden zu einer Datei
-- zusammengefuehrt. Entstehen sie aus doppelten scene_paths-Namen oder aus
-- einer Datei, die sowohl Video als auch Scene-Datei ist. Video-Angaben haben
-- Vorrang. Jede Zusammenfuehrung wird in stage.reject protokolliert.

CREATE TABLE stage.file_candidate (
    source        text   NOT NULL,
    video_id      text,
    host_id       text   NOT NULL,
    mount_path    text   NOT NULL,
    relative_path text   NOT NULL,
    size          bigint,
    sha256        text
);

WITH paths AS (
    SELECT 'video'::text AS source, v.id::text AS video_id,
           stage.uuid_v5('hosts', lower(a.hostname)) AS host_id,
           v.file_path AS p, v.file_size AS size, v.sha256_checksum AS sha256
    FROM src.videos v JOIN src.agents a ON a.id = v.agent_id
    UNION ALL
    SELECT 'scene', NULL, stage.uuid_v5('hosts', 'nuc'),
           sp.name || '/' || sf.name, sf.size, NULL
    FROM src.scene_files sf JOIN src.scene_paths sp ON sp.id = sf.path_id
), mounts AS (
    SELECT paths.*, CASE
        WHEN p ~ '^[A-Za-z]:\\' THEN substring(p FROM '^[A-Za-z]:\\')
        WHEN p ~ '^/mnt/ssd-local/peter/' THEN '/mnt/ssd-local/peter'
        WHEN p ~ '^/(mnt|media)/[^/]+/peter/' THEN substring(p FROM '^/(?:mnt|media)/[^/]+/peter')
        WHEN p ~ '^/home/peter/' THEN '/home/peter'
    END AS mount
    FROM paths
)
INSERT INTO stage.file_candidate
SELECT m.source, m.video_id, m.host_id, m.mount,
       CASE WHEN right(m.mount, 2) = ':\'
            THEN replace(substr(m.p, length(m.mount) + 1), '\', '/')
            ELSE substr(m.p, length(m.mount) + 2) END,
       m.size, m.sha256
FROM mounts m;

-- Protokoll der Zusammenfuehrungen (vor dem Auswaehlen gezaehlt).
INSERT INTO stage.reject
SELECT 'file', c.host_id || '|' || c.mount_path || '|' || c.relative_path,
       'Dublette zusammengefuehrt (' || count(*) || ' Zeilen, 1 Datei)'
FROM stage.file_candidate c
GROUP BY c.host_id, c.mount_path, c.relative_path
HAVING count(*) > 1;

CREATE TABLE stage.volume (
    "ID"             text         PRIMARY KEY,
    "IsActiveEntity" boolean      NOT NULL,
    name             varchar(512) NOT NULL,
    mount_path       varchar(512) NOT NULL,
    host_id          text         NOT NULL,
    UNIQUE (host_id, mount_path)
);

INSERT INTO stage.volume
SELECT DISTINCT stage.uuid_v5('volumes', c.host_id || '|' || c.mount_path), true,
       c.mount_path, c.mount_path, c.host_id
FROM stage.file_candidate c;

CREATE TABLE stage.file (
    "ID"             text         PRIMARY KEY,
    "IsActiveEntity" boolean      NOT NULL,
    name             varchar(512) NOT NULL,
    relative_path    varchar(1024) NOT NULL,
    size             bigint,
    sha256_checksum  varchar(64),
    volume_id        text         NOT NULL,
    video_id         text,
    UNIQUE (volume_id, relative_path)
);

INSERT INTO stage.file
SELECT DISTINCT ON (c.host_id, c.mount_path, c.relative_path)
       stage.uuid_v5('files', c.host_id || '|' || c.mount_path || '|' || c.relative_path), true,
       regexp_replace(c.relative_path, '^.*/', ''),
       c.relative_path, c.size, c.sha256,
       stage.uuid_v5('volumes', c.host_id || '|' || c.mount_path),
       c.video_id
FROM stage.file_candidate c
ORDER BY c.host_id, c.mount_path, c.relative_path, (c.source <> 'video'), c.video_id;
