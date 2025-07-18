pub trait Storage {
    fn get_int(&self) -> u64;
    fn set_int(&self, value: u64);
} 