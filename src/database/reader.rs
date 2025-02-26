use std::path::Path;

use anyhow::{Context, Result};
use deadpool::managed::{Object, Pool};
use libsql::{de::from_row, params};

use super::{
    pool::{Manager, new_pool},
    types::Block,
};

#[derive(Clone)]
pub struct Reader {
    pool: Pool<Manager>,
}

impl Reader {
    pub async fn new(path: &Path) -> Result<Self> {
        let pool = new_pool(path).await?;
        Ok(Self { pool })
    }

    async fn get_connection(&self) -> Result<Object<Manager>> {
        self.pool
            .get()
            .await
            .context("Failed to get connection for database reader pool")
    }

    pub async fn get_last_block(&self) -> Result<Option<Block>> {
        let conn = self.get_connection().await?;
        let mut rows = conn
            .query(
                "SELECT height, hash FROM blocks ORDER BY height DESC LIMIT 1",
                params![],
            )
            .await?;
        Ok(match rows.next().await? {
            Some(row) => Some(from_row::<Block>(&row)?),
            None => None,
        })
    }

    pub async fn get_block_at_height(&self, height: u64) -> Result<Option<Block>> {
        let conn = self.get_connection().await?;
        let mut rows = conn
            .query(
                "SELECT height, hash FROM blocks WHERE height = ?",
                params![height],
            )
            .await?;
        Ok(match rows.next().await? {
            Some(row) => Some(from_row::<Block>(&row)?),
            None => None,
        })
    }
}
