use super::storage_interface::Storage;

static mut INT_REF: u64 = 0;

pub struct MemoryStorage;

impl MemoryStorage {
    pub fn new() -> Self {
        Self
    }
}

impl Storage for MemoryStorage {
    fn get_int(&self) -> u64 {
        unsafe { INT_REF }
    }

    fn set_int(&self, value: u64) {
        unsafe { INT_REF = value }
    }
} 