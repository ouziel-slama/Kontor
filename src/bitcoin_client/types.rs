use std::fmt;

use bitcoin::{Network, Txid};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: Vec<Value>,
}

#[derive(Deserialize, Debug)]
pub struct Response {
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub id: String,
}

fn deserialize_bip70_network<'de, D>(deserializer: D) -> Result<Network, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct NetworkVisitor;
    impl serde::de::Visitor<'_> for NetworkVisitor {
        type Value = Network;

        fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Self::Value, E> {
            Network::from_core_arg(s).map_err(|_| {
                E::invalid_value(
                    serde::de::Unexpected::Str(s),
                    &"bitcoin network encoded as a string",
                )
            })
        }

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "bitcoin network encoded as a string")
        }
    }

    deserializer.deserialize_str(NetworkVisitor)
}

// https://github.com/rust-bitcoin/rust-bitcoincore-rpc/blob/master/json/src/lib.rs#L1016C1-L1058C2
// removed some unused properties that would require copying more types and functions
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetBlockchainInfoResult {
    /// Current network name as defined in BIP70 (main, test, signet, regtest)
    #[serde(deserialize_with = "deserialize_bip70_network")]
    pub chain: Network,
    /// The current number of blocks processed in the server
    pub blocks: u64,
    /// The current number of headers we have validated
    pub headers: u64,
    /// The current difficulty
    pub difficulty: f64,
    /// Median time for the current best block
    #[serde(rename = "mediantime")]
    pub median_time: u64,
    /// Estimate of verification progress [0..1]
    #[serde(rename = "verificationprogress")]
    pub verification_progress: f64,
    /// Estimate of whether this node is in Initial Block Download mode
    #[serde(rename = "initialblockdownload")]
    pub initial_block_download: bool,
    /// The estimated size of the block and undo files on disk
    pub size_on_disk: u64,
    /// If the blocks are subject to pruning
    pub pruned: bool,
    /// Lowest-height complete block stored (only present if pruning is enabled)
    #[serde(rename = "pruneheight")]
    pub prune_height: Option<u64>,
    /// Whether automatic pruning is enabled (only present if pruning is enabled)
    pub automatic_pruning: Option<bool>,
    /// The target size used by pruning (only present if automatic pruning is enabled)
    pub prune_target_size: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TestMempoolAcceptResult {
    pub txid: Txid,
    #[serde(default)]
    pub allowed: Option<bool>,
    pub vsize: Option<u64>,
    pub fees: Option<Fees>,
    #[serde(rename = "reject-reason")]
    pub reject_reason: Option<String>,
    #[serde(rename = "wtxid")]
    pub wtxid: Option<Txid>,
    #[serde(rename = "other-reject-reason")]
    pub other_reject_reason: Option<String>,
    #[serde(rename = "tx-size")]
    pub tx_size: Option<u64>,
    #[serde(rename = "tx-weight")]
    pub tx_weight: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Fees {
    #[serde(rename = "base")]
    pub base: Option<f64>,
    #[serde(rename = "modified")]
    pub modified: Option<f64>,
    #[serde(rename = "ancestor")]
    pub ancestor: Option<f64>,
    #[serde(rename = "descendant")]
    pub descendant: Option<f64>,
}
