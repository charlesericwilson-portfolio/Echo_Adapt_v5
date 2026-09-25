use rusqlite::{Connection, Result};
use std::path::PathBuf;

pub struct ToolDatabase {
    conn: Connection,
}

impl ToolDatabase {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create table if it doesn't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tool_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                session_name TEXT NOT NULL,
                command TEXT NOT NULL,
                summary TEXT
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    /// Log a tool call + summarizer output
    pub fn log_tool_call(
        &self,
        session_name: &str,
        command: &str,
        summary: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tool_history (session_name, command, summary) VALUES (?1, ?2, ?3)",
            [session_name, command, summary],
        )?;
        Ok(())
    }
}
