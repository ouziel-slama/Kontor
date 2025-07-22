pub trait ReadStorage {
    fn get_str(&self, path: &str) -> Option<String>;
    fn get_u64(&self, path: &str) -> Option<u64>;
    fn exists(&self, path: &str) -> bool;
}

pub trait WriteStorage {
    fn set_str(&self, path: &str, value: &str);
    fn set_u64(&self, path: &str, value: u64);
}

pub trait ReadWriteStorage: ReadStorage + WriteStorage {}
