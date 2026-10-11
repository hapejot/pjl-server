//! `video-store`: Ein-Schuss-Migration der Video-Datenbank (Postgres) in ein
//! PJL-Modell mit PostgreSQL-Speicher, serviert als OData v4 (CRUD mit Drafts).
//!
//! # Ablauf
//!
//! 1. `video-ddic`   liest den Katalog und schreibt `ddic.json`.
//! 2. `video-import` baut dasselbe Modell aus der Quelldatenbank und schreibt die
//!    Zeilen in den `PostgresStore` der Zieldatenbank (Parent vor Kind).
//! 3. `video-runtime` serviert das Ergebnis aus derselben Zieldatenbank
//!    (`STORE_PG_URL`).
//!
//! Die Quelldatenbank wird nur lesend benutzt. Ziel ist eine eigene
//! PostgreSQL-Instanz, siehe `video-store/compose.yaml`.
//!
//! # Abbildung
//!
//! * Tabelle -> Entitaet `{Singular}`, Set `{Singular}Set`.
//! * Jede Entitaet hat ein Feld `name`: die vorhandene Spalte `name`, sonst die
//!   Quellspalte aus [`map::NAME_SOURCE`] (umbenannt), sonst aus den Schluesseln abgeleitet.
//! * `uuid`-PK -> `ID` unveraendert. Anderer PK -> `ID` = UUIDv5 aus
//!   (Tabelle, Schluessel), Original als `legacy_id`. Zusammengesetzter oder
//!   fehlender PK -> UUIDv5 aus allen PK-Werten bzw. der Zeile.
//! * Fremdschluessel -> Relationship. `ON DELETE CASCADE` auf NOT-NULL-Spalte
//!   -> Komposition (pro Kind hoechstens eine); sonst Assoziation.
//!   Navigation `video_id` -> `video`, Sammelnavigation `_VideoTags`.
//! * Implizite Fremdschluessel ohne Constraint: [`map::IMPLICIT_FKS`].
//! * `double precision`/`real` -> `Edm.Double` (als Dezimaltext gespeichert).
//! * `jsonb`, Enums, `vector(n)` -> `Edm.String` als JSON- bzw. Text-Wert.
//!   pgvector-Suche ist nicht moeglich.
//!
//! # Bewusst verloren (nicht im DDIC abbildbar)
//!
//! * `NOT NULL`, alle `UNIQUE`-Indizes, `CHECK`-Constraints, Performance-Indizes.
//! * Die Views (`tag_stats`, `unified_video_tags`, `video_with_tags`, `video_stats`).
//! * `ON DELETE SET NULL` wird zur reinen Assoziation.
//! * Die Cascade-Kette `Video -> VideoJob` (`video_jobs` haengt am Agent).
//! * Eindeutigkeit bei zusammengesetzten und fehlenden Schluesseln.
//! * `$apply`/Aggregation und sinnvolles `$search` existieren in diesem Repo nicht.
//!
//! # Typen im Ziel
//!
//! Die Markierung `TODO(pg-backend)` steht bei den Typabbildungen, die im
//! Store als Text liegen (Dezimal, jsonb, Enums, vector).

pub mod import;
pub mod introspect;
pub mod map;
pub mod tag_index;
pub mod tags;
pub mod thumbnails;
