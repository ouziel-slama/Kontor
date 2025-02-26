use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Env {
    pub bitcoin_rpc_url: Option<String>,
    pub bitcoin_rpc_user: Option<String>,
    pub bitcoin_rpc_password: Option<String>,
}
