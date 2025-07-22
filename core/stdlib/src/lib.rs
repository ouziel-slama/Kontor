mod dot_path_buf;
mod storage_interface;

pub use dot_path_buf::*;
pub use storage_interface::*;

pub struct Map<K: ToString, V: Store> {
    _k: std::marker::PhantomData<K>,
    _v: std::marker::PhantomData<V>,
}
