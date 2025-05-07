use axum::extract::{Path, Query, State};

use crate::database::{
    queries::{select_block_at_height, select_block_latest},
    types::BlockRow,
};

use super::{
    Env,
    compose::{ComposeInputs, ComposeOutputs, ComposeQuery, compose},
    error::HttpError,
    result::Result,
};

pub async fn get_block(State(env): State<Env>, Path(height): Path<u64>) -> Result<BlockRow> {
    match select_block_at_height(&*env.reader.connection().await?, height).await? {
        Some(block_row) => Ok(block_row.into()),
        None => Err(HttpError::NotFound(format!("block at height: {}", height)).into()),
    }
}

pub async fn get_block_latest(State(env): State<Env>) -> Result<BlockRow> {
    match select_block_latest(&*env.reader.connection().await?).await? {
        Some(block_row) => Ok(block_row.into()),
        None => Err(HttpError::NotFound("No blocks written".to_owned()).into()),
    }
}

pub async fn get_compose(
    Query(query): Query<ComposeQuery>,
    State(env): State<Env>,
) -> Result<ComposeOutputs> {
    let inputs = ComposeInputs::from_query(query).await?;

    let outputs = compose(inputs)?;

    Ok(outputs.into())
}
