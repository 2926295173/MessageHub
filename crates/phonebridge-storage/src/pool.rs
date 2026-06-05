//! Connection pool + migration runner + all DB queries.

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

    /// Insert or update a device row. The row's `id` (INTEGER PRIMARY KEY)
    /// is left to SQLite's auto-increment on first insert and kept
    /// unchanged on subsequent upserts; the unique key is `device_id`.
    pub async fn upsert_device(&self, d: &DeviceRow) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO devices (name, device_id, public_key, last_seen, paired)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(device_id) DO UPDATE SET
                name = excluded.name,
                public_key = excluded.public_key,
                last_seen = excluded.last_seen
            "#,
        )
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

    /// Update only the `last_seen` column (cheap heartbeat update).
    pub async fn touch_device(&self, device_id: uuid::Uuid, last_seen: i64) -> Result<(), DbError> {
        sqlx::query("UPDATE devices SET last_seen = ?2 WHERE device_id = ?1")
            .bind(device_id)
            .bind(last_seen)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ========================================================================
    // pairings
    // ========================================================================

    /// Insert a pairing row.
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

    /// Mark a device as paired.
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

    /// List the most recent notifications.
    pub async fn list_notifications(
        &self,
        device_id: Option<uuid::Uuid>,
        limit: i64,
        only_unread: bool,
        package_filter: Option<&str>,
    ) -> Result<Vec<NotificationRow>, DbError> {
        // Build query dynamically. For SQLx, we use a static string with
        // conditional WHERE clauses via CASE expressions.
        let sql = if only_unread && device_id.is_some() && package_filter.is_some() {
            "SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read \
             FROM notifications WHERE device_id = ?1 AND read = 0 AND package_name = ?2 \
             ORDER BY posted_at DESC LIMIT ?3"
        } else if only_unread && device_id.is_some() {
            "SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read \
             FROM notifications WHERE device_id = ?1 AND read = 0 \
             ORDER BY posted_at DESC LIMIT ?2"
        } else if only_unread && package_filter.is_some() {
            "SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read \
             FROM notifications WHERE read = 0 AND package_name = ?1 \
             ORDER BY posted_at DESC LIMIT ?2"
        } else if device_id.is_some() && package_filter.is_some() {
            "SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read \
             FROM notifications WHERE device_id = ?1 AND package_name = ?2 \
             ORDER BY posted_at DESC LIMIT ?3"
        } else if device_id.is_some() {
            "SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read \
             FROM notifications WHERE device_id = ?1 \
             ORDER BY posted_at DESC LIMIT ?2"
        } else if package_filter.is_some() {
            "SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read \
             FROM notifications WHERE package_name = ?1 \
             ORDER BY posted_at DESC LIMIT ?2"
        } else if only_unread {
            "SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read \
             FROM notifications WHERE read = 0 \
             ORDER BY posted_at DESC LIMIT ?1"
        } else {
            "SELECT id, device_id, package_name, app_name, title, content, posted_at, is_sensitive, category, read \
             FROM notifications ORDER BY posted_at DESC LIMIT ?1"
        };

        let q = sqlx::query_as::<_, NotificationRow>(sql);
        let rows = match (only_unread, device_id, package_filter) {
            (true, Some(d), Some(p)) => q.bind(d).bind(p).bind(limit).fetch_all(&self.pool).await?,
            (true, Some(d), None) => q.bind(d).bind(limit).fetch_all(&self.pool).await?,
            (true, None, Some(p)) => q.bind(p).bind(limit).fetch_all(&self.pool).await?,
            (false, Some(d), Some(p)) => q.bind(d).bind(p).bind(limit).fetch_all(&self.pool).await?,
            (false, Some(d), None) => q.bind(d).bind(limit).fetch_all(&self.pool).await?,
            (false, None, Some(p)) => q.bind(p).bind(limit).fetch_all(&self.pool).await?,
            (true, None, None) => q.bind(limit).fetch_all(&self.pool).await?,
            (false, None, None) => q.bind(limit).fetch_all(&self.pool).await?,
        };
        Ok(rows)
    }

    /// Mark a single notification as read.
    pub async fn mark_notification_read(
        &self,
        device_id: uuid::Uuid,
        notification_id: &str,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE notifications SET read = 1 WHERE device_id = ?1 AND id = ?2")
            .bind(device_id)
            .bind(notification_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Mark a notification as dismissed on the daemon side and
    /// return the row's `id` (which is the Android
    /// `StatusBarNotification.key`) so the caller can broadcast a
    /// `notification.dismissed` envelope to the device. Returns
    /// None if the row doesn't exist.
    pub async fn dismiss_notification(
        &self,
        device_id: uuid::Uuid,
        notification_id: &str,
    ) -> Result<Option<String>, DbError> {
        use sqlx::Row;
        let row = sqlx::query(
            "UPDATE notifications SET read = 1 \
             WHERE device_id = ?1 AND id = ?2 \
             RETURNING id",
        )
        .bind(device_id)
        .bind(notification_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get::<String, _>("id")))
    }

    /// Count unread notifications across all (or one) device.
    pub async fn count_unread_notifications(
        &self,
        device_id: Option<uuid::Uuid>,
    ) -> Result<i64, DbError> {
        use sqlx::Row;
        let row = if let Some(d) = device_id {
            sqlx::query("SELECT COUNT(*) AS n FROM notifications WHERE read = 0 AND device_id = ?1")
                .bind(d)
                .fetch_one(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT COUNT(*) AS n FROM notifications WHERE read = 0")
                .fetch_one(&self.pool)
                .await?
        };
        let n: i64 = row.try_get("n")?;
        Ok(n)
    }

    /// Count of notifications grouped by package.
    pub async fn count_notifications_by_package(
        &self,
        device_id: Option<uuid::Uuid>,
    ) -> Result<Vec<(String, i64)>, DbError> {
        use sqlx::Row;
        let rows = if let Some(d) = device_id {
            sqlx::query("SELECT package_name, COUNT(*) AS n FROM notifications WHERE device_id = ?1 GROUP BY package_name ORDER BY n DESC")
                .bind(d)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT package_name, COUNT(*) AS n FROM notifications GROUP BY package_name ORDER BY n DESC")
                .fetch_all(&self.pool)
                .await?
        };
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let p: String = r.try_get("package_name")?;
            let n: i64 = r.try_get("n")?;
            out.push((p, n));
        }
        Ok(out)
    }

    // ========================================================================
    // sms_messages
    // ========================================================================

    /// Insert a SMS row.
    pub async fn insert_sms(&self, s: &SmsRow) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO sms_messages
                (id, device_id, sim_slot, phone_number, body, direction, timestamp)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&s.id)
        .bind(s.device_id)
        .bind(s.sim_slot)
        .bind(&s.phone_number)
        .bind(&s.body)
        .bind(&s.direction)
        .bind(s.timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// List SMS, newest first. Optional device + phone-number filter.
    pub async fn list_sms(
        &self,
        device_id: Option<uuid::Uuid>,
        phone_number: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SmsRow>, DbError> {
        // Same pattern as list_notifications.
        let sql = match (device_id.is_some(), phone_number.is_some()) {
            (true, true) => {
                "SELECT id, device_id, sim_slot, phone_number, body, direction, timestamp \
                 FROM sms_messages WHERE device_id = ?1 AND phone_number = ?2 \
                 ORDER BY timestamp DESC LIMIT ?3"
            }
            (true, false) => {
                "SELECT id, device_id, sim_slot, phone_number, body, direction, timestamp \
                 FROM sms_messages WHERE device_id = ?1 \
                 ORDER BY timestamp DESC LIMIT ?2"
            }
            (false, true) => {
                "SELECT id, device_id, sim_slot, phone_number, body, direction, timestamp \
                 FROM sms_messages WHERE phone_number = ?1 \
                 ORDER BY timestamp DESC LIMIT ?2"
            }
            (false, false) => {
                "SELECT id, device_id, sim_slot, phone_number, body, direction, timestamp \
                 FROM sms_messages ORDER BY timestamp DESC LIMIT ?1"
            }
        };
        let q = sqlx::query_as::<_, SmsRow>(sql);
        let rows = match (device_id, phone_number) {
            (Some(d), Some(p)) => q.bind(d).bind(p).bind(limit).fetch_all(&self.pool).await?,
            (Some(d), None) => q.bind(d).bind(limit).fetch_all(&self.pool).await?,
            (None, Some(p)) => q.bind(p).bind(limit).fetch_all(&self.pool).await?,
            (None, None) => q.bind(limit).fetch_all(&self.pool).await?,
        };
        Ok(rows)
    }

    /// Group SMS by phone number, returning (address, last_timestamp, count).
    pub async fn list_sms_conversations(
        &self,
        device_id: Option<uuid::Uuid>,
    ) -> Result<Vec<(String, i64, i64)>, DbError> {
        use sqlx::Row;
        let rows = if let Some(d) = device_id {
            sqlx::query(
                "SELECT phone_number, MAX(timestamp) AS last_ts, COUNT(*) AS n \
                 FROM sms_messages WHERE device_id = ?1 \
                 GROUP BY phone_number ORDER BY last_ts DESC",
            )
            .bind(d)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT phone_number, MAX(timestamp) AS last_ts, COUNT(*) AS n \
                 FROM sms_messages \
                 GROUP BY phone_number ORDER BY last_ts DESC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let p: String = r.try_get("phone_number")?;
            let last: i64 = r.try_get("last_ts")?;
            let n: i64 = r.try_get("n")?;
            out.push((p, last, n));
        }
        Ok(out)
    }

    // ========================================================================
    // calls
    // ========================================================================

    /// Insert a call row.
    pub async fn insert_call(&self, c: &CallRow) -> Result<i64, DbError> {
        let id = sqlx::query(
            r#"
            INSERT INTO calls
                (device_id, phone_number, contact_name, state, started_at, ended_at, direction, duration_secs, sim_slot)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
        )
        .bind(c.device_id)
        .bind(&c.phone_number)
        .bind(&c.contact_name)
        .bind(&c.state)
        .bind(c.started_at)
        .bind(c.ended_at)
        .bind(&c.direction)
        .bind(c.duration_secs)
        .bind(c.sim_slot)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();
        Ok(id)
    }

    /// List calls, newest first.
    pub async fn list_calls(
        &self,
        device_id: Option<uuid::Uuid>,
        limit: i64,
    ) -> Result<Vec<CallRow>, DbError> {
        let sql = if device_id.is_some() {
            "SELECT id, device_id, phone_number, contact_name, state, started_at, ended_at, direction, duration_secs, sim_slot \
             FROM calls WHERE device_id = ?1 \
             ORDER BY started_at DESC LIMIT ?2"
        } else {
            "SELECT id, device_id, phone_number, contact_name, state, started_at, ended_at, direction, duration_secs, sim_slot \
             FROM calls ORDER BY started_at DESC LIMIT ?1"
        };
        let q = sqlx::query_as::<_, CallRow>(sql);
        let rows = if let Some(d) = device_id {
            q.bind(d).bind(limit).fetch_all(&self.pool).await?
        } else {
            q.bind(limit).fetch_all(&self.pool).await?
        };
        Ok(rows)
    }

    // ========================================================================
    // audit_log
    // ========================================================================

    /// Append an audit log row.
    pub async fn insert_audit_log(
        &self,
        timestamp: i64,
        device_id: Option<uuid::Uuid>,
        event: &str,
        detail: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO audit_log (timestamp, device_id, event, detail) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(timestamp)
        .bind(device_id)
        .bind(event)
        .bind(detail)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// List recent audit log rows, newest first.
    pub async fn list_audit_log(&self, limit: i64) -> Result<Vec<AuditLogRow>, DbError> {
        let rows = sqlx::query_as::<_, AuditLogRow>(
            "SELECT id, timestamp, device_id, event, detail FROM audit_log ORDER BY timestamp DESC LIMIT ?1",
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
        let list = db.list_notifications(None, 10, false, None).await.unwrap();
        assert_eq!(list.len(), 1);

        // Filter unread.
        let list = db.list_notifications(None, 10, true, None).await.unwrap();
        assert_eq!(list.len(), 1);
        db.mark_notification_read(dev_id, "notif-1").await.unwrap();
        let list = db.list_notifications(None, 10, true, None).await.unwrap();
        assert_eq!(list.len(), 0);

        // SMS round-trip.
        let s = SmsRow {
            id: "sms-1".into(),
            device_id: dev_id,
            sim_slot: Some(0),
            phone_number: "+8613800000000".into(),
            body: "hello".into(),
            direction: "in".into(),
            timestamp: 1717000001,
        };
        db.insert_sms(&s).await.unwrap();
        let list = db.list_sms(None, None, 10).await.unwrap();
        assert_eq!(list.len(), 1);
        let conv = db.list_sms_conversations(None).await.unwrap();
        assert_eq!(conv.len(), 1);
        assert_eq!(conv[0].0, "+8613800000000");

        // Calls round-trip.
        let c = CallRow {
            id: 0,
            device_id: dev_id,
            phone_number: "+8613800000000".into(),
            contact_name: None,
            state: "ringing".into(),
            started_at: 1717000002,
            ended_at: None,
            direction: "incoming".into(),
            duration_secs: None,
            sim_slot: Some(0),
        };
        let id = db.insert_call(&c).await.unwrap();
        assert!(id > 0);
        let list = db.list_calls(None, 10).await.unwrap();
        assert_eq!(list.len(), 1);

        // Audit log round-trip.
        db.insert_audit_log(1717000099, Some(dev_id), "pair.success", Some("{}")).await.unwrap();
        let log = db.list_audit_log(10).await.unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].event, "pair.success");
    }
}
