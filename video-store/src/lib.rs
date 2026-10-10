//! `video-store`: Ein-Schuss-Migration der Video-Datenbank (Postgres) in ein
//! PJL-Modell mit SQLite-Speicher, serviert als OData v4 (CRUD mit Drafts).
//!
//! # Ablauf
//!
//! 1. `video-ddic`   liest den Katalog und schreibt `ddic.json`.
//! 2. `video-import` baut dasselbe Modell aus der Datenbank und schreibt die
//!    Zeilen in den `RelationalStore` (Parent vor Kind).
//! 3. `pjl-runtime --ddic ddic.json --data-dir <dir>` serviert das Ergebnis.
//!
//! Die Quelldatenbank wird nur lesend benutzt.
//!
//! # Abbildung
//!
//! * Tabelle -> Entitaet `{Singular}`, Set `{Singular}Set`.
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
//! # Pg-Backend (spaeter)
//!
//! Die Markierung `TODO(pg-backend)` steht bei den Typabbildungen, die nur
//! wegen des SQLite-Textspeichers flach sind (jsonb, Enums, vector).

pub mod import;
pub mod introspect;
pub mod map;
