//! Masix Storage
//!
//! SQLite event persistence with ChaCha20-Poly1305 encryption

use anyhow::{anyhow, Result};
use rusqlite::OptionalExtension;
use std::path::Path;
use std::str::FromStr;

pub struct Storage {
    conn: rusqlite::Connection,
}

impl Storage {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = rusqlite::Connection::open(db_path.as_ref())?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel TEXT NOT NULL,
                message_id TEXT NOT NULL,
                chat_id TEXT,
                from_user TEXT,
                content TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            
            CREATE TABLE IF NOT EXISTS secrets (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL
            );
            
            CREATE TABLE IF NOT EXISTS chat_policies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id TEXT NOT NULL UNIQUE,
                policy_type TEXT NOT NULL,
                value TEXT,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            
            CREATE TABLE IF NOT EXISTS automation_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                pattern_type TEXT,
                pattern_value TEXT,
                action_type TEXT,
                action_config TEXT,
                enabled INTEGER DEFAULT 1
            );
            
            CREATE TABLE IF NOT EXISTS channel_offsets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel TEXT NOT NULL,
                account_tag TEXT NOT NULL,
                offset_value INTEGER NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            DELETE FROM channel_offsets
            WHERE id NOT IN (
                SELECT MAX(id)
                FROM channel_offsets
                GROUP BY channel, account_tag
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_channel_offsets_unique
            ON channel_offsets(channel, account_tag);
            
            CREATE TABLE IF NOT EXISTS cron_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_by TEXT NOT NULL,
                schedule TEXT NOT NULL,
                channel TEXT NOT NULL,
                recipient TEXT NOT NULL,
                account_tag TEXT NOT NULL DEFAULT '__default__',
                message TEXT NOT NULL,
                timezone TEXT DEFAULT '+00:00',
                recurring INTEGER DEFAULT 0,
                enabled INTEGER DEFAULT 1,
                last_run DATETIME,
                next_run DATETIME,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;

        Self::ensure_cron_schema(&conn)?;

        Ok(Self { conn })
    }

    pub fn store_event(
        &self,
        channel: &str,
        message_id: &str,
        chat_id: Option<&str>,
        from_user: Option<&str>,
        content: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (channel, message_id, chat_id, from_user, content) VALUES (?1, ?2, ?3, ?4, ?5)",
            (channel, message_id, chat_id, from_user, content),
        )?;
        Ok(())
    }

    pub fn store_secret(&self, key: &str, value: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO secrets (key, value) VALUES (?1, ?2)",
            (key, value),
        )?;
        Ok(())
    }

    pub fn get_secret(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM secrets WHERE key = ?1")?;
        let value: Option<Vec<u8>> = stmt.query_row([key], |row| row.get(0)).optional()?;
        Ok(value)
    }

    pub fn save_offset(&self, channel: &str, account_tag: &str, offset: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO channel_offsets (channel, account_tag, offset_value)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(channel, account_tag)
             DO UPDATE SET offset_value = excluded.offset_value, updated_at = CURRENT_TIMESTAMP",
            (channel, account_tag, offset),
        )?;
        Ok(())
    }

    pub fn get_offset(&self, channel: &str, account_tag: &str) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT offset_value FROM channel_offsets WHERE channel = ?1 AND account_tag = ?2 LIMIT 1"
        )?;
        let offset: Option<i64> = stmt
            .query_row([channel, account_tag], |row| row.get(0))
            .optional()?;
        Ok(offset)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_cron_job(
        &self,
        created_by: &str,
        schedule: &str,
        channel: &str,
        recipient: &str,
        account_tag: Option<&str>,
        message: &str,
        timezone: &str,
        recurring: bool,
    ) -> Result<i64> {
        let account_tag = match account_tag.map(str::trim) {
            Some(value) if !value.is_empty() => value.to_string(),
            _ => "__default__".to_string(),
        };

        let mut stmt = self.conn.prepare(
            "INSERT INTO cron_jobs (created_by, schedule, channel, recipient, account_tag, message, timezone, recurring, next_run)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        )?;

        let next_run = self.compute_next_run(schedule, timezone)?;

        stmt.execute((
            created_by,
            schedule,
            channel,
            recipient,
            account_tag,
            message,
            timezone,
            if recurring { 1 } else { 0 },
            next_run,
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_due_cron_jobs(&self, now: &str) -> Result<Vec<CronJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, created_by, schedule, channel, recipient, account_tag, message, timezone, recurring
             FROM cron_jobs
             WHERE enabled = 1 AND next_run <= ?1",
        )?;

        let jobs = stmt.query_map([now], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                created_by: row.get(1)?,
                schedule: row.get(2)?,
                channel: row.get(3)?,
                recipient: row.get(4)?,
                account_tag: row.get(5)?,
                message: row.get(6)?,
                timezone: row.get(7)?,
                recurring: row.get(8)?,
            })
        })?;

        let mut result = Vec::new();
        for job in jobs {
            result.push(job?);
        }

        Ok(result)
    }

    pub fn list_enabled_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, created_by, schedule, channel, recipient, account_tag, message, timezone, recurring
             FROM cron_jobs
             WHERE enabled = 1
             ORDER BY id DESC",
        )?;

        let jobs = stmt.query_map([], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                created_by: row.get(1)?,
                schedule: row.get(2)?,
                channel: row.get(3)?,
                recipient: row.get(4)?,
                account_tag: row.get(5)?,
                message: row.get(6)?,
                timezone: row.get(7)?,
                recurring: row.get(8)?,
            })
        })?;

        let mut result = Vec::new();
        for job in jobs {
            result.push(job?);
        }
        Ok(result)
    }

    pub fn list_enabled_cron_jobs_for_account(&self, account_tag: &str) -> Result<Vec<CronJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, created_by, schedule, channel, recipient, account_tag, message, timezone, recurring
             FROM cron_jobs
             WHERE enabled = 1 AND account_tag = ?1
             ORDER BY id DESC",
        )?;

        let jobs = stmt.query_map([account_tag], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                created_by: row.get(1)?,
                schedule: row.get(2)?,
                channel: row.get(3)?,
                recipient: row.get(4)?,
                account_tag: row.get(5)?,
                message: row.get(6)?,
                timezone: row.get(7)?,
                recurring: row.get(8)?,
            })
        })?;

        let mut result = Vec::new();
        for job in jobs {
            result.push(job?);
        }
        Ok(result)
    }

    pub fn list_enabled_cron_jobs_for_account_recipient(
        &self,
        account_tag: &str,
        recipient: &str,
    ) -> Result<Vec<CronJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, created_by, schedule, channel, recipient, account_tag, message, timezone, recurring
             FROM cron_jobs
             WHERE enabled = 1 AND account_tag = ?1 AND recipient = ?2
             ORDER BY id DESC",
        )?;

        let jobs = stmt.query_map((account_tag, recipient), |row| {
            Ok(CronJob {
                id: row.get(0)?,
                created_by: row.get(1)?,
                schedule: row.get(2)?,
                channel: row.get(3)?,
                recipient: row.get(4)?,
                account_tag: row.get(5)?,
                message: row.get(6)?,
                timezone: row.get(7)?,
                recurring: row.get(8)?,
            })
        })?;

        let mut result = Vec::new();
        for job in jobs {
            result.push(job?);
        }
        Ok(result)
    }

    pub fn count_enabled_cron_jobs(&self) -> Result<i64> {
        let count = self.conn.query_row(
            "SELECT COUNT(*) FROM cron_jobs WHERE enabled = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn count_enabled_cron_jobs_for_account(&self, account_tag: &str) -> Result<i64> {
        let count = self.conn.query_row(
            "SELECT COUNT(*) FROM cron_jobs WHERE enabled = 1 AND account_tag = ?1",
            [account_tag],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn update_cron_next_run(&self, id: i64, schedule: &str, timezone: &str) -> Result<()> {
        let next_run = self.compute_next_run(schedule, timezone)?;
        self.conn.execute(
            "UPDATE cron_jobs SET next_run = ?1, last_run = CURRENT_TIMESTAMP WHERE id = ?2",
            (next_run, id),
        )?;
        Ok(())
    }

    pub fn disable_cron_job(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE cron_jobs SET enabled = 0 WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn disable_cron_job_for_account(&self, id: i64, account_tag: &str) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE cron_jobs SET enabled = 0 WHERE id = ?1 AND account_tag = ?2",
            (id, account_tag),
        )?;
        Ok(changed > 0)
    }

    fn ensure_cron_schema(conn: &rusqlite::Connection) -> Result<()> {
        let mut has_account_tag = false;
        let mut stmt = conn.prepare("PRAGMA table_info(cron_jobs)")?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for col in columns {
            if col?.eq_ignore_ascii_case("account_tag") {
                has_account_tag = true;
                break;
            }
        }

        if !has_account_tag {
            conn.execute(
                "ALTER TABLE cron_jobs ADD COLUMN account_tag TEXT NOT NULL DEFAULT '__default__'",
                [],
            )?;
        }

        conn.execute(
            "UPDATE cron_jobs
             SET account_tag = '__default__'
             WHERE account_tag IS NULL OR TRIM(account_tag) = ''",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cron_jobs_due_account
             ON cron_jobs(enabled, next_run, account_tag)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cron_jobs_account_enabled
             ON cron_jobs(account_tag, enabled)",
            [],
        )?;

        Ok(())
    }

    fn compute_next_run(&self, schedule: &str, timezone: &str) -> Result<String> {
        let schedule = schedule.trim();
        let _timezone = timezone;

        if schedule
            .split_whitespace()
            .any(|part| part == "*" || part.contains('/'))
        {
            let cron_expr = match schedule.split_whitespace().count() {
                5 => format!("0 {}", schedule),
                6 | 7 => schedule.to_string(),
                _ => return Err(anyhow!("Invalid cron expression: {}", schedule)),
            };

            let parsed = cron::Schedule::from_str(&cron_expr)
                .map_err(|e| anyhow!("Invalid cron expression '{}': {}", schedule, e))?;
            let next = parsed
                .after(&chrono::Utc::now())
                .next()
                .ok_or_else(|| anyhow!("No next execution for cron expression '{}'", schedule))?;

            Ok(next.to_rfc3339())
        } else {
            match chrono::DateTime::parse_from_rfc3339(schedule) {
                Ok(dt) => Ok(dt.with_timezone(&chrono::Utc).to_rfc3339()),
                Err(_) => Ok(schedule.to_string()),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CronJob {
    pub id: i64,
    pub created_by: String,
    pub schedule: String,
    pub channel: String,
    pub recipient: String,
    pub account_tag: String,
    pub message: String,
    pub timezone: String,
    pub recurring: bool,
}

#[cfg(test)]
mod tests {
    use super::Storage;
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("masix-storage-{}-{}.db", name, ts))
    }

    #[test]
    fn create_and_filter_cron_jobs_by_account_tag() {
        let path = temp_db_path("scope");
        let storage = Storage::new(&path).expect("storage init");
        let now = chrono::Utc::now().to_rfc3339();

        storage
            .create_cron_job(
                "test",
                &now,
                "telegram",
                "100",
                Some("bot_a"),
                "A",
                "+00:00",
                false,
            )
            .expect("insert A");
        storage
            .create_cron_job(
                "test",
                &now,
                "telegram",
                "200",
                Some("bot_b"),
                "B",
                "+00:00",
                false,
            )
            .expect("insert B");

        let a_jobs = storage
            .list_enabled_cron_jobs_for_account("bot_a")
            .expect("query A");
        let b_jobs = storage
            .list_enabled_cron_jobs_for_account("bot_b")
            .expect("query B");

        assert_eq!(a_jobs.len(), 1);
        assert_eq!(b_jobs.len(), 1);
        assert_eq!(a_jobs[0].account_tag, "bot_a");
        assert_eq!(b_jobs[0].account_tag, "bot_b");
    }

    #[test]
    fn migrates_legacy_cron_table_with_default_account_tag() {
        let path = temp_db_path("legacy");
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch(
            "
            CREATE TABLE cron_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_by TEXT NOT NULL,
                schedule TEXT NOT NULL,
                channel TEXT NOT NULL,
                recipient TEXT NOT NULL,
                message TEXT NOT NULL,
                timezone TEXT DEFAULT '+00:00',
                recurring INTEGER DEFAULT 0,
                enabled INTEGER DEFAULT 1,
                last_run DATETIME,
                next_run DATETIME,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO cron_jobs (created_by, schedule, channel, recipient, message, timezone, recurring, enabled, next_run)
            VALUES ('legacy', '2099-01-01T00:00:00Z', 'telegram', '123', 'legacy-msg', '+00:00', 0, 1, '2099-01-01T00:00:00Z');
            ",
        )
        .expect("seed legacy");
        drop(conn);

        let storage = Storage::new(&path).expect("migrated storage");
        let jobs = storage.list_enabled_cron_jobs().expect("list");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].account_tag, "__default__");
    }
}
