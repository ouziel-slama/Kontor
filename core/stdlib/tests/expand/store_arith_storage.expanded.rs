pub struct ArithStorage {
    pub last_op: Option<Op>,
}
#[automatically_derived]
impl stdlib::Store for ArithStorage {
    fn __set(
        ctx: &impl stdlib::WriteContext,
        base_path: stdlib::DotPathBuf,
        value: ArithStorage,
    ) {
        ctx.__set(base_path.push("last_op"), value.last_op);
    }
}
