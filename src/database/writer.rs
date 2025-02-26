use std::path::Path;

use anyhow::Result;
use libsql::Connection;

use super::{connection::new_connection, types::Block};

#[derive(Clone)]
pub struct Writer {
    conn: Connection,
}

impl Writer {
    pub async fn new(path: &Path) -> Result<Self> {
        let conn = new_connection(path).await?;
        Ok(Self { conn })
    }

    pub async fn insert_block(&self, block: Block) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO blocks (height, hash) VALUES (?, ?)",
                (block.height, block.hash.to_string()),
            )
            .await?;
        Ok(())
    }
}
