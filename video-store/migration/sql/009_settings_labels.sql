-- 009 Einstellungen und Labels: global_thumbnail_settings, global_snapshot_positions,
-- label_categories, label_category_options.
-- label_categories: ID = Quell-Schluessel (key), legacy_id = key.
-- label_category_options.category_key -> category_ID (Referenz auf LabelCategory).

CREATE TABLE stage.global_thumbnail_setting (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    legacy_id        text        NOT NULL,
    name             text        NOT NULL,
    updated_at       timestamptz NOT NULL
);

INSERT INTO stage.global_thumbnail_setting
SELECT stage.uuid_v5('global_thumbnail_settings', g.id::text), true, g.id::text, g.id::text, g.updated_at
FROM src.global_thumbnail_settings g;

CREATE TABLE stage.global_snapshot_position (
    "ID"             text             PRIMARY KEY,
    "IsActiveEntity" boolean          NOT NULL,
    name             text             NOT NULL,
    position_index   integer          NOT NULL,
    position_type    text             NOT NULL,
    value            double precision NOT NULL,
    created_at       timestamptz      NOT NULL
);

INSERT INTO stage.global_snapshot_position
SELECT p.id::text, true, p.id::text, p.position_index, p.position_type::text,
       p.value, p.created_at
FROM src.global_snapshot_positions p;

CREATE TABLE stage.label_category (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    legacy_id        text        NOT NULL,
    name             text        NOT NULL,
    sort_order       integer     NOT NULL,
    created_at       timestamptz NOT NULL
);

INSERT INTO stage.label_category
SELECT stage.uuid_v5('label_categories', c.key), true, c.key, c.name, c.sort_order, c.created_at
FROM src.label_categories c;

CREATE TABLE stage.label_category_option (
    "ID"             text        PRIMARY KEY,
    "IsActiveEntity" boolean     NOT NULL,
    name             text        NOT NULL,
    value            text        NOT NULL,
    sort_order       integer     NOT NULL,
    created_at       timestamptz NOT NULL,
    category_key     text        NOT NULL,
    UNIQUE (category_key, value)
);

INSERT INTO stage.label_category_option
SELECT o.id::text, true, o.id::text, o.value, o.sort_order, o.created_at,
       stage.uuid_v5('label_categories', o.category_key)
FROM src.label_category_options o;
