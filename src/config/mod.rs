mod env;

use anyhow::{Result, anyhow};
use env::Env;
use serde::{Deserialize, Serialize};
use serde_env::from_env;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub bitcoin_rpc_url: String,
    pub bitcoin_rpc_user: String,
    pub bitcoin_rpc_password: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let env: Env = from_env()?;
        Ok(Self {
            bitcoin_rpc_url: env
                .bitcoin_rpc_url
                .ok_or(anyhow!("BITCOIN_RPC_URL not set"))?,
            bitcoin_rpc_user: env
                .bitcoin_rpc_user
                .ok_or(anyhow!("BITCOIN_RPC_USER not set"))?,
            bitcoin_rpc_password: env
                .bitcoin_rpc_password
                .ok_or(anyhow!("BITCOIN_RPC_PASSWORD not set"))?,
        })
    }
}
