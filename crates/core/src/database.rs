//! The app's data in SQLite: one file in Application Support, opened by the app, brought up to date by [`MIGRATIONS`].
//! Each migration runs once, in order, recorded in SQLite's `user_version`, so a newer app upgrades an older file and
//! an older app refuses a newer one rather than damage it. Replace `history` with the app's own tables.

use std::path::Path;

use jiff::Timestamp;
use rusqlite::{Connection, params};

/// The schema, a step at a time. Only ever add to the end: a released step has run on someone's data.
const MIGRATIONS: &[&str] = &[
    // 1: what the tally was, and when.
    "CREATE TABLE history (at TEXT NOT NULL, count INTEGER NOT NULL);",
];

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(
        "the data is from a newer version of Patchbay (schema {found}, this one knows {known})"
    )]
    TooNew { found: usize, known: usize },
}

pub struct Database {
    connection: Connection,
}

impl Database {
    /// Opens the database at `path`, making it if need be, and brings it up to date.
    pub fn open(path: &Path) -> Result<Database, DatabaseError> {
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        Database::migrate(Connection::open(path)?)
    }

    /// A database that lives only as long as it's held, for tests.
    pub fn in_memory() -> Result<Database, DatabaseError> {
        Database::migrate(Connection::open_in_memory()?)
    }

    fn migrate(mut connection: Connection) -> Result<Database, DatabaseError> {
        // Several windows, or a second copy of the app, may write at once.
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let found: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let found = usize::try_from(found).unwrap_or(0);
        if found > MIGRATIONS.len() {
            return Err(DatabaseError::TooNew {
                found,
                known: MIGRATIONS.len(),
            });
        }
        for (step, sql) in MIGRATIONS.iter().enumerate().skip(found) {
            let transaction = connection.transaction()?;
            transaction.execute_batch(sql)?;
            transaction.pragma_update(None, "user_version", step as i64 + 1)?;
            transaction.commit()?;
        }
        Ok(Database { connection })
    }

    /// Notes what the tally is now.
    pub fn record(&self, count: u32, at: Timestamp) -> Result<(), DatabaseError> {
        self.connection.execute(
            "INSERT INTO history (at, count) VALUES (?1, ?2)",
            params![at.to_string(), count],
        )?;
        Ok(())
    }

    /// The last `limit` counts recorded, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<(Timestamp, u32)>, DatabaseError> {
        let mut statement = self
            .connection
            .prepare("SELECT at, count FROM history ORDER BY at DESC LIMIT ?1")?;
        let rows = statement.query_map(params![limit as i64], |row| {
            let at: String = row.get(0)?;
            Ok((at.parse().unwrap_or(Timestamp::UNIX_EPOCH), row.get(1)?))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::{Database, DatabaseError, MIGRATIONS};
    use jiff::Timestamp;

    #[test]
    fn what_is_recorded_comes_back_newest_first() {
        let database = Database::in_memory().unwrap();
        let earlier: Timestamp = "2026-11-04T10:00:00Z".parse().unwrap();
        let later: Timestamp = "2026-11-04T10:05:00Z".parse().unwrap();
        database.record(1, earlier).unwrap();
        database.record(2, later).unwrap();
        assert_eq!(database.recent(10).unwrap(), vec![(later, 2), (earlier, 1)]);
    }

    #[test]
    fn a_file_is_migrated_once_and_a_newer_one_is_refused() {
        let path =
            std::env::temp_dir().join(format!("patchbay-db-{}/data.sqlite", std::process::id()));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        Database::open(&path)
            .unwrap()
            .record(3, Timestamp::now())
            .unwrap();
        // Opening again runs nothing twice and keeps the data.
        assert_eq!(Database::open(&path).unwrap().recent(1).unwrap()[0].1, 3);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .unwrap();
        drop(connection);
        assert!(matches!(
            Database::open(&path),
            Err(DatabaseError::TooNew { .. })
        ));
    }
}
