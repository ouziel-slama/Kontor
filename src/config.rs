use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_env::from_env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Env {
    pub bitcoin_rpc_url: Option<String>,
    pub bitcoin_rpc_user: Option<String>,
    pub bitcoin_rpc_password: Option<String>,
    pub zmq_pub_sequence_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub bitcoin_rpc_url: String,
    pub bitcoin_rpc_user: String,
    pub bitcoin_rpc_password: String,
    pub zmq_pub_sequence_address: String,
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
            zmq_pub_sequence_address: env
                .zmq_pub_sequence_address
                .ok_or(anyhow!("ZMQ_PUB_SEQUENCE_ADDRESS not set"))?,
        })
    }
}
