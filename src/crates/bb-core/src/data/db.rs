//! Connection factory and migration runner. Migrations are `include_str!` files applied in order;
//! `PRAGMA user_version` tracks the last applied number. Spec: Core/Data/Db.cs.

use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("migrations/001_schema.sql"))];

/// The schema version this build's migrations produce.
pub const EXPECTED_SCHEMA_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].0;

const POOL_SIZE: usize = 4;

pub struct Db {
    path: PathBuf,
    create: bool,
    pool: Mutex<Vec<Connection>>,
}

/// A pooled connection; returns to the pool on drop.
pub struct Conn<'a> {
    db: &'a Db,
    conn: Option<Connection>,
}

impl Deref for Conn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("connection present")
    }
}

impl DerefMut for Conn<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn.as_mut().expect("connection present")
    }
}

impl Drop for Conn<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let mut pool = self.db.pool.lock().unwrap_or_else(|p| p.into_inner());
            if pool.len() < POOL_SIZE {
                pool.push(conn);
            }
        }
    }
}

impl Db {
    /// Opens a database, creating it and its directory when missing.
    pub fn new(path: impl Into<PathBuf>) -> Db {
        Db::with_mode(path.into(), true)
    }

    /// Opens an existing database and refuses to bring one into existence. For the elevated index
    /// helper: a file an elevated process creates is owned by Administrators, so the app creates and
    /// migrates first and the helper attaches to what is there, failing loudly if it is not.
    pub fn attach(path: impl Into<PathBuf>) -> Db {
        Db::with_mode(path.into(), false)
    }

    fn with_mode(path: PathBuf, create: bool) -> Db {
        Db {
            path,
            create,
            pool: Mutex::new(Vec::new()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open(&self) -> rusqlite::Result<Conn<'_>> {
        let pooled = self.pool.lock().unwrap_or_else(|p| p.into_inner()).pop();
        let conn = match pooled {
            Some(c) => c,
            None => self.connect()?,
        };
        Ok(Conn {
            db: self,
            conn: Some(conn),
        })
    }

    fn connect(&self) -> rusqlite::Result<Connection> {
        if self.create
            && let Some(dir) = self.path.parent()
        {
            let _ = std::fs::create_dir_all(dir);
        }
        let flags = if self.create {
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
        };
        let conn = Connection::open_with_flags(&self.path, flags)?;
        conn.busy_timeout(Duration::from_millis(5000))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(conn)
    }

    /// Drops every pooled connection, so the files can be deleted or the process can exit cleanly.
    pub fn clear_pool(&self) {
        self.pool.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    /// The schema this database is at, as the last applied migration's number. Read by the elevated
    /// index helper, which never migrates and exits when this moves.
    pub fn schema_version(&self) -> rusqlite::Result<i64> {
        let conn = self.open()?;
        conn.query_row("PRAGMA user_version;", [], |r| r.get(0))
    }

    pub fn migrate(&self) -> rusqlite::Result<()> {
        let mut conn = self.open()?;
        let current: i64 = conn.query_row("PRAGMA user_version;", [], |r| r.get(0))?;
        for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {version};"))?;
            tx.commit()?;
        }
        Ok(())
    }
}
