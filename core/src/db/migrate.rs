use rusqlite::Connection;

use crate::error::AppError;

/// Applies a list of `(name, sql)` migrations to `conn`, tracking which ones
/// have already run in a `schema_migrations` table. Each tool crate owns its
/// own migration list (its SQLite file, its tables) and calls this generic
/// runner instead of reimplementing the bookkeeping.
pub fn apply_migrations(conn: &Connection, migrations: &[(&str, &str)]) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    for (name, sql) in migrations {
        let already_applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE name = ?1)",
            [name],
            |row| row.get(0),
        )?;
        if already_applied {
            continue;
        }
        conn.execute_batch(sql)?;
        conn.execute("INSERT INTO schema_migrations (name) VALUES (?1)", [name])?;
    }

    Ok(())
}
