use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

use crate::error::AppError;

/// SQLite connection pool wrapper shared by all tool crates.
pub type DbPool = r2d2::Pool<SqliteConnectionManager>;

pub fn create_pool(db_path: &Path) -> Result<DbPool, AppError> {
    let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
        conn.execute_batch("PRAGMA foreign_keys = OFF; PRAGMA journal_mode = WAL;")
    });
    // `min_idle(Some(0))`: don't eagerly warm the pool up to `max_size`
    // connections on creation. Desktop tools typically open several small
    // per-purpose SQLite files at startup; lazily creating connections keeps
    // that quiet instead of several pools racing to write a WAL header on a
    // freshly created empty file at once.
    r2d2::Pool::builder()
        .max_size(8)
        .min_idle(Some(0))
        .build(manager)
        .map_err(|e| AppError::Database(e.to_string()))
}
