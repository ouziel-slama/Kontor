use crate::runtime::Runtime;
use crate::testlib_exports::*;

import!(
    name = "storage_agreement",
    mod_name = "api",
    height = 0,
    tx_index = 0,
    path = "../../native-contracts/storage-agreement/wit",
    public = true,
);

pub fn address() -> ContractAddress {
    ContractAddress {
        name: "storage_agreement".to_string(),
        height: 0,
        tx_index: 0,
    }
}
