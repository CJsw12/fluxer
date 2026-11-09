// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::config;
use crate::contracts::AccountEngineSnapshotCursor;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS rows(account_key TEXT, entity TEXT, key TEXT, value TEXT,
	PRIMARY KEY(account_key, entity, key)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS cursors(account_key TEXT PRIMARY KEY, session_id TEXT, schema_epoch INTEGER, updated_at INTEGER);
CREATE TABLE IF NOT EXISTS summaries(account_key TEXT PRIMARY KEY, json TEXT, updated_at INTEGER);
CREATE TABLE IF NOT EXISTS mentions(account_key TEXT, message_id TEXT, created_at INTEGER, json TEXT,
	PRIMARY KEY(account_key, message_id)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS mentions_by_time ON mentions(account_key, created_at DESC, message_id DESC);
"#;

pub struct SqliteStore {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    db_path: Option<PathBuf>,
}

impl SqliteStore {
    /// 기본 디렉터리(`%APPDATA%/fluxer_desktop_rs/storage/account-engine-cache.sqlite3`)에 SQLite DB를 열거나 생성합니다.
    pub fn open_default() -> Result<Self> {
        let storage_dir = config::get_config_dir().join("storage");
        let _ = std::fs::create_dir_all(&storage_dir);
        let db_path = storage_dir.join("account-engine-cache.sqlite3");
        Self::open_path(&db_path)
    }

    /// 지정된 파일 경로로 SQLite DB를 열고 스키마 및 WAL 모드를 초기화합니다.
    pub fn open_path(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("Failed to open sqlite database at {}", path.display()))?;

        Self::init_connection(conn, Some(path.to_path_buf()))
    }

    /// 테스트용 순수 인메모리 SQLite DB를 생성합니다.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("Failed to open in-memory sqlite database")?;
        Self::init_connection(conn, None)
    }

    fn init_connection(conn: Connection, db_path: Option<PathBuf>) -> Result<Self> {
        // WAL 모드 및 성능 프라그마 설정
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;",
        )
        .context("Failed to configure sqlite pragmas")?;

        // 공식 스키마 적용
        conn.execute_batch(SCHEMA)
            .context("Failed to apply sqlite schema")?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
        })
    }

    // =========================================================================
    // Meta 테이블 메서드
    // =========================================================================

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            let val: String = row.get(0)?;
            Ok(Some(val))
        } else {
            Ok(None)
        }
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // =========================================================================
    // Rows 테이블 메서드 (AccountEngine Row Store)
    // =========================================================================

    pub fn get_row(&self, account_key: &str, entity: &str, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT value FROM rows WHERE account_key = ?1 AND entity = ?2 AND key = ?3",
        )?;
        let mut rows = stmt.query(params![account_key, entity, key])?;
        if let Some(row) = rows.next()? {
            let val: String = row.get(0)?;
            Ok(Some(val))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_row(&self, account_key: &str, entity: &str, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO rows(account_key, entity, key, value) VALUES (?1, ?2, ?3, ?4)",
            params![account_key, entity, key, value],
        )?;
        Ok(())
    }

    pub fn delete_row(&self, account_key: &str, entity: &str, key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM rows WHERE account_key = ?1 AND entity = ?2 AND key = ?3",
            params![account_key, entity, key],
        )?;
        Ok(())
    }

    pub fn delete_rows_by_prefix(&self, account_key: &str, entity: &str, prefix: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("{prefix}%");
        conn.execute(
            "DELETE FROM rows WHERE account_key = ?1 AND entity = ?2 AND key LIKE ?3",
            params![account_key, entity, pattern],
        )?;
        Ok(())
    }

    pub fn rows_by_prefix(
        &self,
        account_key: &str,
        entity: &str,
        prefix: &str,
    ) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("{prefix}%");
        let mut stmt = conn.prepare_cached(
            "SELECT key, value FROM rows WHERE account_key = ?1 AND entity = ?2 AND key LIKE ?3 ORDER BY key ASC",
        )?;
        let mut rows = stmt.query(params![account_key, entity, pattern])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let k: String = row.get(0)?;
            let v: String = row.get(1)?;
            results.push((k, v));
        }
        Ok(results)
    }

    pub fn replace_entity(
        &self,
        account_key: &str,
        entity: &str,
        entries: &[(String, String)],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            tx.execute(
                "DELETE FROM rows WHERE account_key = ?1 AND entity = ?2",
                params![account_key, entity],
            )?;
            let mut stmt = tx.prepare_cached(
                "INSERT INTO rows(account_key, entity, key, value) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (k, v) in entries {
                stmt.execute(params![account_key, entity, k, v])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_account_rows(&self, account_key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM rows WHERE account_key = ?1",
            params![account_key],
        )?;
        Ok(())
    }

    // =========================================================================
    // Cursors 테이블 메서드
    // =========================================================================

    pub fn read_cursor(&self, account_key: &str) -> Result<Option<AccountEngineSnapshotCursor>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT session_id, schema_epoch, updated_at FROM cursors WHERE account_key = ?1",
        )?;
        let mut rows = stmt.query(params![account_key])?;
        if let Some(row) = rows.next()? {
            let session_id: String = row.get(0)?;
            let schema_epoch: u32 = row.get(1)?;
            let updated_at: u64 = row.get(2)?;
            Ok(Some(AccountEngineSnapshotCursor {
                session_id,
                schema_epoch,
                updated_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn write_cursor(&self, account_key: &str, cursor: &AccountEngineSnapshotCursor) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO cursors(account_key, session_id, schema_epoch, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![account_key, cursor.session_id, cursor.schema_epoch, cursor.updated_at],
        )?;
        Ok(())
    }

    pub fn delete_cursor(&self, account_key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM cursors WHERE account_key = ?1",
            params![account_key],
        )?;
        Ok(())
    }

    // =========================================================================
    // Summaries 테이블 메서드
    // =========================================================================

    pub fn all_summaries(&self) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT json FROM summaries ORDER BY updated_at DESC")?;
        let mut rows = stmt.query([])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let json_str: String = row.get(0)?;
            if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
                results.push(val);
            }
        }
        Ok(results)
    }

    pub fn set_summary(&self, account_key: &str, summary_json: &str, updated_at: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO summaries(account_key, json, updated_at) VALUES (?1, ?2, ?3)",
            params![account_key, summary_json, updated_at],
        )?;
        Ok(())
    }

    pub fn delete_summary(&self, account_key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM summaries WHERE account_key = ?1",
            params![account_key],
        )?;
        Ok(())
    }

    // =========================================================================
    // Mentions 테이블 메서드
    // =========================================================================

    pub fn list_mentions(&self, account_key: &str, limit: u32) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT json FROM mentions WHERE account_key = ?1 ORDER BY created_at DESC, message_id DESC LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![account_key, limit])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let json_str: String = row.get(0)?;
            if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
                results.push(val);
            }
        }
        Ok(results)
    }

    pub fn insert_mention(
        &self,
        account_key: &str,
        message_id: &str,
        created_at: u64,
        mention_json: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO mentions(account_key, message_id, created_at, json) VALUES (?1, ?2, ?3, ?4)",
            params![account_key, message_id, created_at, mention_json],
        )?;
        Ok(())
    }

    pub fn delete_mention(&self, account_key: &str, message_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM mentions WHERE account_key = ?1 AND message_id = ?2",
            params![account_key, message_id],
        )?;
        Ok(())
    }

    pub fn delete_mentions(&self, account_key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM mentions WHERE account_key = ?1",
            params![account_key],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_store_lifecycle() {
        let store = SqliteStore::open_in_memory().expect("open in-memory");

        // Meta 검증
        store.set_meta("schema_epoch", "1").unwrap();
        assert_eq!(store.get_meta("schema_epoch").unwrap().as_deref(), Some("1"));

        // Rows 검증
        store.upsert_row("acc1", "channels", "chan_100", r#"{"name":"general"}"#).unwrap();
        store.upsert_row("acc1", "channels", "chan_101", r#"{"name":"dev"}"#).unwrap();
        assert_eq!(
            store.get_row("acc1", "channels", "chan_100").unwrap().as_deref(),
            Some(r#"{"name":"general"}"#)
        );

        let prefix_rows = store.rows_by_prefix("acc1", "channels", "chan_").unwrap();
        assert_eq!(prefix_rows.len(), 2);

        // Replace entity 검증
        let new_entries = vec![("chan_200".to_string(), r#"{"name":"random"}"#.to_string())];
        store.replace_entity("acc1", "channels", &new_entries).unwrap();
        assert_eq!(store.get_row("acc1", "channels", "chan_100").unwrap(), None);
        assert_eq!(
            store.get_row("acc1", "channels", "chan_200").unwrap().as_deref(),
            Some(r#"{"name":"random"}"#)
        );

        // Cursor 검증
        let cursor = AccountEngineSnapshotCursor {
            session_id: "sess_xyz".to_string(),
            schema_epoch: 1,
            updated_at: 1700000000,
        };
        store.write_cursor("acc1", &cursor).unwrap();
        let read_back = store.read_cursor("acc1").unwrap().expect("cursor should exist");
        assert_eq!(read_back.session_id, "sess_xyz");
        assert_eq!(read_back.schema_epoch, 1);
        assert_eq!(read_back.updated_at, 1700000000);

        // Summaries 검증
        store.set_summary("acc1", r#"{"accountKey":"acc1","unreadCount":3}"#, 1700000000).unwrap();
        let summaries = store.all_summaries().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0]["accountKey"], "acc1");
        assert_eq!(summaries[0]["unreadCount"], 3);

        // Mentions 검증
        store.insert_mention("acc1", "msg_1", 1000, r#"{"messageId":"msg_1","content":"hello"}"#).unwrap();
        store.insert_mention("acc1", "msg_2", 2000, r#"{"messageId":"msg_2","content":"world"}"#).unwrap();
        let mentions = store.list_mentions("acc1", 10).unwrap();
        assert_eq!(mentions.len(), 2);
        assert_eq!(mentions[0]["messageId"], "msg_2"); // DESC 정렬 검증
    }
}
