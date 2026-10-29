//! SQL-Tracing fuer den Postgres-Store.
//!
//! Jede Anweisung (query, query_one, execute, batch_execute, Transaktions-Start
//! und -Commit sowie das Holen einer Verbindung aus dem Pool) wird mit Dauer,
//! Ergebnis und dem SQL-Text geloggt. Parameterwerte werden bewusst nicht
//! ausgegeben, nur ihre Anzahl.
//!
//! Aktivieren mit `RUST_LOG=sql=debug` (erfolgreiche Statements), fehlgeschlagene
//! Statements werden immer als WARN geloggt. `RUST_LOG=sql=trace` zusaetzlich
//! erfolgreiche Pool-Entnahmen.

use std::time::Instant;

use deadpool_postgres::{ClientWrapper, GenericClient, Object, Pool, PoolError, Transaction};
use tokio_postgres::{types::ToSql, Error, Row};
use tracing::{debug, trace, warn};

const TARGET: &str = "sql";

/// Zeilenumbrueche und Mehrfach-Leerzeichen entfernen, damit eine Zeile im Log bleibt.
fn compact(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn log_statement(op: &'static str, sql: &str, started: Instant, params: usize, rows: Option<usize>, err: Option<String>) {
    let duration_ms = ms(started);
    let sql = compact(sql);
    match err {
        None => debug!(
            target: TARGET,
            op,
            duration_ms,
            rows = rows.unwrap_or(0),
            params,
            sql = %sql,
            "sql ok"
        ),
        Some(error) => warn!(
            target: TARGET,
            op,
            duration_ms,
            params,
            sql = %sql,
            error = %error,
            "sql failed"
        ),
    }
}

pub async fn query<C: GenericClient>(
    conn: &C,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Vec<Row>, Error> {
    let started = Instant::now();
    let r = conn.query(sql, params).await;
    log_statement(
        "query",
        sql,
        started,
        params.len(),
        r.as_ref().ok().map(|v| v.len()),
        r.as_ref().err().map(|e| e.to_string()),
    );
    r
}

pub async fn query_one<C: GenericClient>(
    conn: &C,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Row, Error> {
    let started = Instant::now();
    let r = conn.query_one(sql, params).await;
    log_statement(
        "query_one",
        sql,
        started,
        params.len(),
        r.as_ref().ok().map(|_| 1),
        r.as_ref().err().map(|e| e.to_string()),
    );
    r
}

pub async fn execute<C: GenericClient>(
    conn: &C,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<u64, Error> {
    let started = Instant::now();
    let r = conn.execute(sql, params).await;
    log_statement(
        "execute",
        sql,
        started,
        params.len(),
        r.as_ref().ok().map(|n| *n as usize),
        r.as_ref().err().map(|e| e.to_string()),
    );
    r
}

pub async fn batch_execute(conn: &Object, sql: &str) -> Result<(), Error> {
    let started = Instant::now();
    let r = conn.batch_execute(sql).await;
    log_statement(
        "batch_execute",
        sql,
        started,
        0,
        None,
        r.as_ref().err().map(|e| e.to_string()),
    );
    r
}

/// Holt eine Verbindung aus dem Pool und misst die Wartezeit.
pub async fn conn(pool: &Pool) -> Result<Object, PoolError> {
    let started = Instant::now();
    let r = pool.get().await;
    match &r {
        Ok(_) => trace!(target: TARGET, op = "pool.get", duration_ms = ms(started), "Verbindung aus Pool"),
        Err(e) => warn!(target: TARGET, op = "pool.get", duration_ms = ms(started), error = %e, "Pool-Fehler"),
    }
    r
}

pub async fn begin(conn: &mut ClientWrapper) -> Result<Transaction<'_>, Error> {
    let started = Instant::now();
    let r = conn.transaction().await;
    log_statement("begin", "BEGIN", started, 0, None, r.as_ref().err().map(|e| e.to_string()));
    r
}

pub async fn commit(tx: Transaction<'_>) -> Result<(), Error> {
    let started = Instant::now();
    let r = tx.commit().await;
    log_statement("commit", "COMMIT", started, 0, None, r.as_ref().err().map(|e| e.to_string()));
    r
}
