use crate::storage_interface::ReadWriteStorage;

pub mod storage_interface;

pub fn store_and_return_int<S: ReadWriteStorage>(storage: &S, path: &str, x: u64) -> u64 {
    storage.set_u64(path, x);

    let retrieved = storage.get_u64(path);
    retrieved.unwrap_or(0)
}
