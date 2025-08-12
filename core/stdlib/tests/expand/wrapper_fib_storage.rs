use stdlib::Wrapper;

#[derive(Wrapper)]
struct FibValue {
    pub value: u64,
}

#[derive(Wrapper)]
struct FibStorage {
    pub cache: Map<u64, FibValue>,
}
