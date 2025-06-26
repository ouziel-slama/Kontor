use super::{storage::Storage, wit::kontor::contract::built_in::Host};
use anyhow::Result;

pub struct Context {
    storage: Storage,
}

impl Context {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }
}

impl Host for Context {
    async fn set(&mut self, key: String, value: Vec<u8>) -> Result<()> {
        self.storage.set(&key, &value).await
    }

    async fn get(&mut self, key: String) -> Result<Option<Vec<u8>>> {
        self.storage.get(&key).await
    }

    async fn delete(&mut self, key: String) -> Result<bool> {
        self.storage.delete(&key).await
    }
}
