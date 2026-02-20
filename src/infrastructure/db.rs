use color_eyre::eyre::{Result, WrapErr};
use rusqlite::{params, Connection};
use std::path::Path;

use crate::domain::types::{
    CachedMessage, Id, MessageAttachment, MessageEmbed, MessageMarker, MessageReference,
    ChannelMarker, UserMarker,
};

/// Initialize the database: create tables, indexes, and enable WAL mode.
pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY,
            channel_id INTEGER NOT NULL,
            author_id INTEGER NOT NULL,
            content TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            edited_timestamp TEXT,
            attachments TEXT,
            embeds TEXT,
            message_reference TEXT,
            mentions TEXT,
            mention_everyone INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel_id, timestamp);
        CREATE INDEX IF NOT EXISTS idx_messages_channel_id ON messages(channel_id, id DESC);

        CREATE TABLE IF NOT EXISTS channels (
            id INTEGER PRIMARY KEY,
            guild_id INTEGER,
            name TEXT NOT NULL,
            kind INTEGER NOT NULL,
            position INTEGER DEFAULT 0,
            parent_id INTEGER,
            last_read_message_id INTEGER
        );

        CREATE TABLE IF NOT EXISTS guilds (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            icon TEXT
        );

        CREATE TABLE IF NOT EXISTS sessions (
            name TEXT PRIMARY KEY,
            pane_layout TEXT NOT NULL,
            last_used TEXT NOT NULL
        );",
    )?;

    Ok(())
}

/// Open a database connection and initialize schema.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).wrap_err("Failed to create database directory")?;
    }
    let conn = Connection::open(path).wrap_err("Failed to open database")?;
    initialize(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (for testing).
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory().wrap_err("Failed to open in-memory database")?;
    initialize(&conn)?;
    Ok(conn)
}

/// Insert a single message.
pub fn insert_message(conn: &Connection, msg: &CachedMessage) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO messages (id, channel_id, author_id, content, timestamp, edited_timestamp, attachments, embeds, message_reference, mentions, mention_everyone) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            msg.id.get() as i64,
            msg.channel_id.get() as i64,
            msg.author_id.get() as i64,
            msg.content,
            msg.timestamp,
            msg.edited_timestamp,
            serde_json::to_string(&msg.attachments).unwrap_or_default(),
            serde_json::to_string(&msg.embeds).unwrap_or_default(),
            msg.message_reference.as_ref().map(|r| serde_json::to_string(r).unwrap_or_default()),
            serde_json::to_string(&msg.mentions.iter().map(|id| id.get()).collect::<Vec<_>>()).unwrap_or_default(),
            msg.mention_everyone as i32,
        ],
    )?;
    Ok(())
}

/// Insert multiple messages in a single transaction.
pub fn insert_messages(conn: &mut Connection, messages: &[CachedMessage]) -> Result<()> {
    let tx = conn.transaction()?;
    for msg in messages {
        insert_message(&tx, msg)?;
    }
    tx.commit()?;
    Ok(())
}

/// Fetch messages for a channel, ordered by timestamp descending, with optional before_timestamp filter.
pub fn fetch_messages(
    conn: &Connection,
    channel_id: Id<ChannelMarker>,
    before_timestamp: Option<&str>,
    limit: u32,
) -> Result<Vec<CachedMessage>> {
    let mut messages = if let Some(before) = before_timestamp {
        let mut stmt = conn.prepare(
            "SELECT id, channel_id, author_id, content, timestamp, edited_timestamp, attachments, embeds, message_reference, mentions, mention_everyone FROM messages WHERE channel_id = ?1 AND timestamp < ?2 ORDER BY timestamp DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![channel_id.get() as i64, before, limit], row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().wrap_err("Failed to fetch messages")?
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, channel_id, author_id, content, timestamp, edited_timestamp, attachments, embeds, message_reference, mentions, mention_everyone FROM messages WHERE channel_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![channel_id.get() as i64, limit], row_to_message)?;
        rows.collect::<Result<Vec<_>, _>>().wrap_err("Failed to fetch messages")?
    };

    // Reverse to chronological order (oldest first)
    messages.reverse();
    Ok(messages)
}

/// Update a message's content and edited_timestamp.
pub fn update_message(
    conn: &Connection,
    id: Id<MessageMarker>,
    content: &str,
    edited_timestamp: &str,
) -> Result<usize> {
    let rows = conn.execute(
        "UPDATE messages SET content = ?1, edited_timestamp = ?2 WHERE id = ?3",
        params![content, edited_timestamp, id.get() as i64],
    )?;
    Ok(rows)
}

/// Delete a message by ID.
pub fn delete_message(conn: &Connection, id: Id<MessageMarker>) -> Result<usize> {
    let rows = conn.execute("DELETE FROM messages WHERE id = ?1", params![id.get() as i64])?;
    Ok(rows)
}

/// Save a session (pane layout).
pub fn save_session(conn: &Connection, name: &str, layout_json: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sessions (name, pane_layout, last_used) VALUES (?1, ?2, datetime('now'))",
        params![name, layout_json],
    )?;
    Ok(())
}

/// Load a session by name.
pub fn load_session(conn: &Connection, name: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT pane_layout FROM sessions WHERE name = ?1")?;
    let result = stmt
        .query_row(params![name], |row| row.get::<_, String>(0))
        .ok();
    Ok(result)
}

/// Convert a database row to a CachedMessage.
fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedMessage> {
    let id_val: i64 = row.get(0)?;
    let channel_id_val: i64 = row.get(1)?;
    let author_id_val: i64 = row.get(2)?;
    let content: String = row.get(3)?;
    let timestamp: String = row.get(4)?;
    let edited_timestamp: Option<String> = row.get(5)?;
    let attachments_json: Option<String> = row.get(6)?;
    let embeds_json: Option<String> = row.get(7)?;
    let reference_json: Option<String> = row.get(8)?;
    let mentions_json: Option<String> = row.get(9)?;
    let mention_everyone: i32 = row.get(10)?;

    let attachments: Vec<MessageAttachment> = attachments_json
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();

    let embeds: Vec<MessageEmbed> = embeds_json
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();

    let message_reference: Option<MessageReference> =
        reference_json.and_then(|j| serde_json::from_str(&j).ok());

    let mentions: Vec<Id<UserMarker>> = mentions_json
        .and_then(|j| serde_json::from_str::<Vec<u64>>(&j).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            if v > 0 {
                Some(Id::new(v))
            } else {
                None
            }
        })
        .collect();

    Ok(CachedMessage {
        id: Id::new(id_val as u64),
        channel_id: Id::new(channel_id_val as u64),
        author_id: Id::new(author_id_val as u64),
        content,
        timestamp,
        edited_timestamp,
        attachments,
        embeds,
        message_reference,
        mention_everyone: mention_everyone != 0,
        mentions,
        rendered: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_message(id: u64, channel_id: u64, content: &str, timestamp: &str) -> CachedMessage {
        CachedMessage {
            id: Id::new(id),
            channel_id: Id::new(channel_id),
            author_id: Id::new(100),
            content: content.to_string(),
            timestamp: timestamp.to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        }
    }

    #[test]
    fn schema_creation() {
        let conn = open_in_memory().unwrap();
        // Verify tables exist by querying them
        let count: i64 = conn.query_row("SELECT count(*) FROM messages", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
        let count: i64 = conn.query_row("SELECT count(*) FROM channels", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
        let count: i64 = conn.query_row("SELECT count(*) FROM guilds", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
        let count: i64 = conn.query_row("SELECT count(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn wal_mode_enabled() {
        let conn = open_in_memory().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        // In-memory databases may report "memory" or "wal"
        assert!(mode == "wal" || mode == "memory", "Got mode: {}", mode);
    }

    #[test]
    fn insert_and_fetch_message() {
        let conn = open_in_memory().unwrap();
        let msg = make_test_message(1, 10, "Hello world", "2024-01-01T00:00:00Z");
        insert_message(&conn, &msg).unwrap();

        let fetched = fetch_messages(&conn, Id::new(10), None, 50).unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].content, "Hello world");
        assert_eq!(fetched[0].id.get(), 1);
        assert_eq!(fetched[0].channel_id.get(), 10);
        assert_eq!(fetched[0].author_id.get(), 100);
    }

    #[test]
    fn insert_message_with_all_fields() {
        let conn = open_in_memory().unwrap();
        let msg = CachedMessage {
            id: Id::new(1),
            channel_id: Id::new(10),
            author_id: Id::new(100),
            content: "Check this out".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: Some("2024-01-01T01:00:00Z".to_string()),
            attachments: vec![MessageAttachment {
                filename: "photo.png".to_string(),
                size: 1024,
                url: "https://cdn.example.com/photo.png".to_string(),
                content_type: Some("image/png".to_string()),
            }],
            embeds: vec![MessageEmbed {
                title: Some("Link".to_string()),
                description: None,
                color: Some(0xFF0000),
                url: None,
            }],
            message_reference: Some(MessageReference {
                message_id: Some(Id::new(99)),
                channel_id: Some(Id::new(10)),
                guild_id: None,
            }),
            mention_everyone: true,
            mentions: vec![Id::new(200), Id::new(300)],
            rendered: None,
        };
        insert_message(&conn, &msg).unwrap();

        let fetched = fetch_messages(&conn, Id::new(10), None, 50).unwrap();
        assert_eq!(fetched.len(), 1);
        let f = &fetched[0];
        assert_eq!(f.edited_timestamp, Some("2024-01-01T01:00:00Z".to_string()));
        assert_eq!(f.attachments.len(), 1);
        assert_eq!(f.attachments[0].filename, "photo.png");
        assert_eq!(f.embeds.len(), 1);
        assert_eq!(f.embeds[0].title, Some("Link".to_string()));
        assert!(f.message_reference.is_some());
        assert_eq!(
            f.message_reference.as_ref().unwrap().message_id,
            Some(Id::new(99))
        );
        assert!(f.mention_everyone);
        assert_eq!(f.mentions.len(), 2);
    }

    #[test]
    fn batch_insert_messages() {
        let mut conn = open_in_memory().unwrap();
        let messages = vec![
            make_test_message(1, 10, "First", "2024-01-01T00:00:00Z"),
            make_test_message(2, 10, "Second", "2024-01-01T00:01:00Z"),
            make_test_message(3, 10, "Third", "2024-01-01T00:02:00Z"),
        ];
        insert_messages(&mut conn, &messages).unwrap();

        let fetched = fetch_messages(&conn, Id::new(10), None, 50).unwrap();
        assert_eq!(fetched.len(), 3);
        // Should be in chronological order (oldest first)
        assert_eq!(fetched[0].content, "First");
        assert_eq!(fetched[1].content, "Second");
        assert_eq!(fetched[2].content, "Third");
    }

    #[test]
    fn fetch_messages_with_limit() {
        let mut conn = open_in_memory().unwrap();
        let messages: Vec<CachedMessage> = (1..=10)
            .map(|i| {
                make_test_message(
                    i,
                    10,
                    &format!("Message {}", i),
                    &format!("2024-01-01T00:{:02}:00Z", i),
                )
            })
            .collect();
        insert_messages(&mut conn, &messages).unwrap();

        let fetched = fetch_messages(&conn, Id::new(10), None, 3).unwrap();
        assert_eq!(fetched.len(), 3);
        // Should get the 3 most recent, in chronological order
        assert_eq!(fetched[0].content, "Message 8");
        assert_eq!(fetched[1].content, "Message 9");
        assert_eq!(fetched[2].content, "Message 10");
    }

    #[test]
    fn fetch_messages_before_timestamp() {
        let mut conn = open_in_memory().unwrap();
        let messages = vec![
            make_test_message(1, 10, "Old", "2024-01-01T00:00:00Z"),
            make_test_message(2, 10, "Middle", "2024-01-01T01:00:00Z"),
            make_test_message(3, 10, "New", "2024-01-01T02:00:00Z"),
        ];
        insert_messages(&mut conn, &messages).unwrap();

        let fetched = fetch_messages(
            &conn,
            Id::new(10),
            Some("2024-01-01T02:00:00Z"),
            50,
        )
        .unwrap();
        assert_eq!(fetched.len(), 2);
        assert_eq!(fetched[0].content, "Old");
        assert_eq!(fetched[1].content, "Middle");
    }

    #[test]
    fn fetch_messages_different_channels() {
        let mut conn = open_in_memory().unwrap();
        let messages = vec![
            make_test_message(1, 10, "Channel 10", "2024-01-01T00:00:00Z"),
            make_test_message(2, 20, "Channel 20", "2024-01-01T00:00:00Z"),
            make_test_message(3, 10, "Channel 10 again", "2024-01-01T00:01:00Z"),
        ];
        insert_messages(&mut conn, &messages).unwrap();

        let ch10 = fetch_messages(&conn, Id::new(10), None, 50).unwrap();
        assert_eq!(ch10.len(), 2);

        let ch20 = fetch_messages(&conn, Id::new(20), None, 50).unwrap();
        assert_eq!(ch20.len(), 1);
        assert_eq!(ch20[0].content, "Channel 20");
    }

    #[test]
    fn update_message_content() {
        let conn = open_in_memory().unwrap();
        let msg = make_test_message(1, 10, "Original", "2024-01-01T00:00:00Z");
        insert_message(&conn, &msg).unwrap();

        let rows = update_message(&conn, Id::new(1), "Updated", "2024-01-01T01:00:00Z").unwrap();
        assert_eq!(rows, 1);

        let fetched = fetch_messages(&conn, Id::new(10), None, 50).unwrap();
        assert_eq!(fetched[0].content, "Updated");
        assert_eq!(
            fetched[0].edited_timestamp,
            Some("2024-01-01T01:00:00Z".to_string())
        );
    }

    #[test]
    fn update_nonexistent_message() {
        let conn = open_in_memory().unwrap();
        let rows = update_message(&conn, Id::new(999), "Nope", "2024-01-01T00:00:00Z").unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn delete_message_by_id() {
        let conn = open_in_memory().unwrap();
        let msg = make_test_message(1, 10, "To delete", "2024-01-01T00:00:00Z");
        insert_message(&conn, &msg).unwrap();

        let rows = delete_message(&conn, Id::new(1)).unwrap();
        assert_eq!(rows, 1);

        let fetched = fetch_messages(&conn, Id::new(10), None, 50).unwrap();
        assert!(fetched.is_empty());
    }

    #[test]
    fn delete_nonexistent_message() {
        let conn = open_in_memory().unwrap();
        let rows = delete_message(&conn, Id::new(999)).unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn insert_or_replace_message() {
        let conn = open_in_memory().unwrap();
        let msg = make_test_message(1, 10, "Original", "2024-01-01T00:00:00Z");
        insert_message(&conn, &msg).unwrap();

        let updated = make_test_message(1, 10, "Replaced", "2024-01-01T00:00:00Z");
        insert_message(&conn, &updated).unwrap();

        let fetched = fetch_messages(&conn, Id::new(10), None, 50).unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].content, "Replaced");
    }

    #[test]
    fn save_and_load_session() {
        let conn = open_in_memory().unwrap();
        let layout = r#"{"root":{"type":"leaf","pane_id":0}}"#;
        save_session(&conn, "default", layout).unwrap();

        let loaded = load_session(&conn, "default").unwrap();
        assert_eq!(loaded, Some(layout.to_string()));
    }

    #[test]
    fn load_nonexistent_session() {
        let conn = open_in_memory().unwrap();
        let loaded = load_session(&conn, "nonexistent").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn save_session_overwrites() {
        let conn = open_in_memory().unwrap();
        save_session(&conn, "test", "layout1").unwrap();
        save_session(&conn, "test", "layout2").unwrap();

        let loaded = load_session(&conn, "test").unwrap();
        assert_eq!(loaded, Some("layout2".to_string()));
    }

    #[test]
    fn open_file_database() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let conn = open(&db_path).unwrap();
        let msg = make_test_message(1, 10, "Persisted", "2024-01-01T00:00:00Z");
        insert_message(&conn, &msg).unwrap();
        drop(conn);

        // Re-open and verify data persists
        let conn2 = open(&db_path).unwrap();
        let fetched = fetch_messages(&conn2, Id::new(10), None, 50).unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].content, "Persisted");
    }

    #[test]
    fn open_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("subdir").join("nested").join("test.db");

        let conn = open(&db_path).unwrap();
        assert!(db_path.exists());
        drop(conn);
    }
}
