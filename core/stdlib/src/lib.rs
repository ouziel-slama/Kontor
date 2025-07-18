pub mod memory_storage;
pub mod storage_interface;

use self::storage_interface::Storage;

pub fn store_and_return_int<S: Storage>(storage: &S, x: u64) -> u64 {
    storage.set_int(x);
    storage.get_int()
}
