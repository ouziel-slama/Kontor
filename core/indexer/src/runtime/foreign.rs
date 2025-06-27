use anyhow::Result;

use crate::runtime::Runtime;

#[derive(Clone)]
pub struct Foreign {
    pub contract_id: String,
}

impl Foreign {
    pub fn new(contract_id: String) -> Self {
        Self { contract_id }
    }

    pub async fn call(&self, runtime: Runtime, expr: &str) -> Result<String> {
        runtime.execute(expr).await
    }
}
