//! Connection pool + migration runner.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{SqlitePool, migrate::Migrator};

use crate::models::*;

/// Embedded migrations. Run with [`Db::migrate`].
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// A SQLite connection pool.
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Open a SQLite database at `path`, creating it if necessary.
    pub async fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let opts = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(opts)
            .await?;
        Ok(Self { pool })
    }

    /// Open an in-memory database (for tests).
    pub async fn open_memory() -> Result<Self, DbError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?
            .journal_mode(SqliteJournalMode::Memory)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        Ok(Self { pool })
    }

    /// Run all pending migrations.
    pub async fn migrate(&self) -> Result<(), DbError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    /// Underlying pool (for advanced queries).
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // ========================================================================
    // devices
    // ========================================================================

    /// Insert or update a device row.
    pub async fn upsert_device(&self, d: &DeviceRow) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO devices (id, name, device_id, public_key, last_seen, paired)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(device_id) DO UPDATE SET
                name = excluded.name,
                public_key = excluded.public_key,
                last_seen = excluded.last_seen,
                paired = excluded.paired
            "#,
        )
        .bind(d.id)
        .bind(&d.name)
        .bind(d.device_id)
        .bind(&d.public_key)
        .bind(d.last_seen)
        .bind(d.paired)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// List all devices.
    pub async fn list_devices(&self) -> Result<Vec<DeviceRow>, DbError> {
        let rows = sqlx::query_as::<_, DeviceRow>(
            "SELECT id, name, device_id, public_key, last_seen, paired FROM devices ORDER BY last_seen DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Get a device by its `device_id` (UUIDv4).
    pub async fn get_device(&self, device_id: uuid::Uuid) -> Result<Option<DeviceRow>, DbError> {
        let row = sqlx::query_as::<_, DeviceRow>(
            "SELECT id, name, device_id, public_key, last_seen, paired FROM devices WHERE device_id = ?1",
        )
        .bind(device_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Remove a device by `device_id`.
    pub async fn remove_device(&self, device_id: uuid::Uuid) -> Result<(), DbError> {
        sqlx::query("DELETE FROM devices WHERE device_id = ?1")
            .bind(device_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ========================================================================
    // pairings
    // ========================================================================

    /// Insert a pairing row. Caller is expected to also call
    /// `upsert_device` + `mark_device_paired`.
    pub async fn insert_pairing(
        &self,
        device_id: uuid::Uuid,
        cert_pem: &str,
        cert_fingerprint: &str,
        paired_at: i64,
    ) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO pairings (device_id, cert_pem, cert_fingerprint, paired_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(device_id) DO UPDATE SET
                cert_pem = excluded.cert_pem,
                cert_fingerprint = excluded.cert_fingerprint,
                paired_at = excluded.paired_at
            "#,
        )
        .bind(device_id)
        .bind(cert_pem)
        .bind(cert_fingerprint)
        .bind(paired_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get a pairing row for a device, if any.
    pub async fn get_pairing(
        &self,
        device_id: uuid::Uuid,
    ) -> Result<Option<PairingRow>, DbError> {
        let row = sqlx::query_as::<_, PairingRow>(
            "SELECT id, device_id, cert_pem, cert_fingerprint, paired_at FROM pairings WHERE device_id = ?1",
        )
        .bind(device_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Mark a device as paired (called after a successful pairing completes).
    pub async fn mark_device_paired(&self, device_id: uuid::Uuid) -> Result<(), DbError> {
        sqlx::query("UPDATE devices SET paired = 1 WHERE device_id = ?1")
            .bind(device_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ========================================================================
    // notifications
    // ========================================================================

    /// Insert a notification.
    pub async fn insert_notification(&self, n: &NotificationRow) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO notifications
                (id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(&n.id)
        .bind(n.device_id)
        .bind(&n.package_name)
        .bind(&n.app_name)
        .bind(&n.title)
        .bind(&n.content)
        .bind(n.posted_at)
        .bind(n.is_sensitive)
        .bind(&n.category)
        .bind(n.read)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// List the most recent notifications, newest first.
    pub async fn list_notifications(&self, limit: i64) -> Result<Vec<NotificationRow>, DbError> {
        let rows = sqlx::query_as::<_, NotificationRow>(
            r#"
            SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read
            FROM notifications ORDER BY posted_at DESC LIMIT ?1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

/// Errors from the storage layer.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// SQLx error.
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    /// Migration error.
    #[error("migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// I/O error.
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_migrates_and_inserts() {
        let db = Db::open_memory().await.unwrap();
        db.migrate().await.unwrap();

        // Insert a device.
        let dev = DeviceRow {
            id: 0,
            name: "Pixel 8 Pro".into(),
            device_id: uuid::Uuid::new_v4(),
            public_key: "AAAA".into(),
            last_seen: 1717000000,
            paired: true,
        };
        let dev_id = dev.device_id;
        db.upsert_device(&dev).await.unwrap();

        let got = db.get_device(dev_id).await.unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().name, "Pixel 8 Pro");

        // Insert a notification.
        let n = NotificationRow {
            id: "notif-1".into(),
            device_id: dev_id,
            package_name: "com.test".into(),
            app_name: Some("Test".into()),
            title: "Hi".into(),
            content: "Hello".into(),
            posted_at: 1717000000,
            is_sensitive: false,
            category: None,
            read: false,
        };
        db.insert_notification(&n).await.unwrap();
        let list = db.list_notifications(10).await.unwrap();
        assert_eq!(list.len(), 1);
    }
}
