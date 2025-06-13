use bitcoin::BlockHash;
use libsql::{Connection, de::from_row, params};
use thiserror::Error as ThisError;

use crate::database::types::{
    BlockTransactionCursor, PaginationMeta, TransactionCursor, TransactionResponse, TransactionRow
};

use super::types::{BlockRow, ContractStateRow};

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("LibSQL error: {0}")]
    LibSQL(#[from] libsql::Error),
    #[error("Row deserialization error: {0}")]
    RowDeserialization(#[from] serde::de::value::Error),
    #[error("Invalid cursor: {0}")]
    InvalidCursor(#[from] crate::database::types::Error),
}

pub async fn insert_block(conn: &Connection, block: BlockRow) -> Result<i64, Error> {
    conn.execute(
        "INSERT OR REPLACE INTO blocks (height, hash) VALUES (?, ?)",
        (block.height, block.hash.to_string()),
    )
    .await?;
    Ok(conn.last_insert_rowid())
}

pub async fn rollback_to_height(conn: &Connection, height: u64) -> Result<u64, Error> {
    let num_rows = conn
        .execute("DELETE FROM blocks WHERE height > ?", [height])
        .await?;

    Ok(num_rows)
}

pub async fn select_block_latest(conn: &Connection) -> Result<Option<BlockRow>, Error> {
    let mut rows = conn
        .query(
            "SELECT height, hash FROM blocks ORDER BY height DESC LIMIT 1",
            params![],
        )
        .await?;
    Ok(rows.next().await?.map(|r| from_row(&r)).transpose()?)
}

pub async fn select_block_at_height(
    conn: &Connection,
    height: u64,
) -> Result<Option<BlockRow>, Error> {
    let mut rows = conn
        .query(
            "SELECT height, hash FROM blocks WHERE height = ?",
            params![height],
        )
        .await?;
    Ok(rows.next().await?.map(|r| from_row(&r)).transpose()?)
}

pub async fn select_block_with_hash(
    conn: &Connection,
    hash: &BlockHash,
) -> Result<Option<BlockRow>, Error> {
    let mut rows = conn
        .query(
            "SELECT height, hash FROM blocks WHERE hash = ?",
            params![hash.to_string()],
        )
        .await?;
    Ok(rows.next().await?.map(|r| from_row(&r)).transpose()?)
}

pub async fn insert_contract_state(conn: &Connection, row: ContractStateRow) -> Result<i64, Error> {
    conn.execute(
        r#"
            INSERT OR REPLACE INTO contract_state (
                contract_id,
                tx_id,
                height,
                path,
                value,
                deleted
            ) VALUES (?, ?, ?, ?, ?, ?)
        "#,
        params![
            row.contract_id,
            row.tx_id,
            row.height,
            row.path,
            row.value,
            row.deleted
        ],
    )
    .await?;

    Ok(conn.last_insert_rowid())
}

pub async fn get_latest_contract_state(
    conn: &Connection,
    contract_id: &str,
    path: &str,
) -> Result<Option<ContractStateRow>, Error> {
    let mut rows = conn
        .query(
            r#"
                SELECT
                    id,
                    contract_id,
                    tx_id,
                    height,
                    path,
                    value,
                    deleted
                FROM contract_state
                WHERE contract_id = ? AND path = ?
                ORDER BY height DESC
                LIMIT 1
            "#,
            params![contract_id, path],
        )
        .await?;

    Ok(rows.next().await?.map(|r| from_row(&r)).transpose()?)
}

pub async fn insert_transaction(conn: &Connection, row: TransactionRow) -> Result<i64, Error> {
    conn.execute(
        "INSERT INTO transactions (height, txid, tx_index) VALUES (?, ?, ?)",
        params![row.height, row.txid, row.tx_index],
    )
    .await?;

    Ok(conn.last_insert_rowid())
}

pub async fn get_transaction_by_id(
    conn: &Connection,
    id: i64,
) -> Result<Option<TransactionRow>, Error> {
    let mut rows = conn
        .query(
            "SELECT id, txid, height, tx_index FROM transactions WHERE id = ?",
            params![id],
        )
        .await?;

    Ok(rows.next().await?.map(|r| from_row(&r)).transpose()?)
}

pub async fn get_transaction_by_txid(
    conn: &Connection,
    txid: &str,
) -> Result<Option<TransactionRow>, Error> {
    let mut rows = conn
        .query(
            "SELECT id, txid, height, tx_index FROM transactions WHERE txid = ?",
            params![txid],
        )
        .await?;

    Ok(rows.next().await?.map(|r| from_row(&r)).transpose()?)
}

pub async fn get_transactions_at_height(
    conn: &Connection,
    height: u64,
) -> Result<Vec<TransactionRow>, Error> {
    let mut rows = conn
        .query(
            "SELECT id, txid, height, tx_index FROM transactions WHERE height = ?",
            params![height],
        )
        .await?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        results.push(from_row(&row)?);
    }
    Ok(results)
}
pub async fn get_transactions_paginated(
    conn: &Connection,
    height: Option<u64>,
    cursor: Option<String>,
    offset: Option<u64>,
    limit: u32,
) -> Result<(Vec<TransactionResponse>, PaginationMeta), Error> {
    let mut params = Vec::new();

    // Build height filter for /blocks/height/transactions
    let height_filter_sql = if let Some(h) = height {
        params.push(libsql::Value::Integer(h as i64));
        format!("AND t.height = ?{}", params.len())
    } else {
        String::new()
    };

    // Build cursor filter 
    let cursor_filter_sql = if let Some(cursor_str) = cursor.clone() {
        if height.is_some() {
            // /blocks/height/transactions: decode as tx_index only
            let cursor = BlockTransactionCursor::decode(&cursor_str)
                .map_err(Error::InvalidCursor)?;
            params.push(libsql::Value::Integer(cursor.tx_index as i64));
            format!("AND t.tx_index < ?{}", params.len())
        } else {
            // /transactions: decode as height:tx_index
            let cursor = TransactionCursor::decode(&cursor_str)
                .map_err(Error::InvalidCursor)?;
            params.push(libsql::Value::Integer(cursor.height as i64));
            params.push(libsql::Value::Integer(cursor.tx_index as i64));
            format!(
                "AND ((t.height, t.tx_index) < (?{}, ?{}))",
                params.len() - 1,
                params.len()
            )
        }
    } else {
        String::new()
    };

    // Build OFFSET clause (only if no cursor)
    let offset_clause = if cursor.is_none() {
        if let Some(offset_val) = offset {
            format!("OFFSET {}", offset_val)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let query = format!(
        r#"
        SELECT 
            t.txid,
            t.height,
            t.tx_index,
            (SELECT MAX(height) FROM blocks) as latest_height,
            COUNT(*) OVER() as total_count
        FROM transactions t
        WHERE 1=1
            {height_filter}
            {cursor_filter}
        ORDER BY t.height DESC, t.tx_index DESC
        LIMIT {limit_plus_one}
        {offset_clause}
        "#,
        height_filter = height_filter_sql,
        cursor_filter = cursor_filter_sql,
        limit_plus_one = limit + 1,
        offset_clause = offset_clause
    );

    let mut rows = conn.query(&query, params).await?;

    // make sql query look like the returned results!!!
    let mut transactions = Vec::new();
    let mut latest_height = 0u64;
    let mut total_count = 0u64;
    let mut has_more = false;


    // TODO: clean this all up!!
    while let Some(row) = rows.next().await? {
        let txid: String = row.get(0)?;
        let height: i64 = row.get(1)?;
        let tx_index: i64 = row.get(2)?;
        let latest_height_from_db: i64 = row.get(3)?;
        let total_count_from_db: i64 = row.get(4)?;

        latest_height = latest_height_from_db as u64;
        total_count = total_count_from_db as u64;

        // Check if we have more results than requested
        if transactions.len() >= limit as usize {
            has_more = true;
            break;
        }

        transactions.push(TransactionResponse {
            txid,
            height: height as u64,
            tx_index: tx_index as i32,
        });
    }

    // Generate next cursor/offset based on pagination type
    let (next_cursor, next_offset) = if cursor.is_some() {
        // Cursor-based pagination
        let next_cursor = if has_more && !transactions.is_empty() {
            let last_tx = transactions.last().unwrap();
            let cursor = TransactionCursor {
                height: last_tx.height,
                tx_index: last_tx.tx_index,
            };
            Some(cursor.encode())
        } else {
            None
        };
        (next_cursor, None)
    } else {
        // Offset-based pagination
        match offset {
            Some(current_offset) => {
                let next_offset = if has_more {
                    Some(current_offset + limit as u64)
                } else {
                    None
                };
                (None, next_offset)
            }
            None => {
                // First page with offset-based pagination
                let next_offset = if has_more { Some(limit as u64) } else { None };
                (None, next_offset)
            }
        }
    };

    let pagination_meta = PaginationMeta {
        next_cursor,
        next_offset,
        has_more,
        latest_height,
        total_count: Some(total_count),
    };

    Ok((transactions, pagination_meta))
}
