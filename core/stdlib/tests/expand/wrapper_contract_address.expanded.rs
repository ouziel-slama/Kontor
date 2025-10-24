use stdlib::WrapperNext;
pub struct ContractAddress {
    pub name: String,
    pub height: i64,
    pub tx_index: i64,
}
pub struct ContractAddressWrapperNext<'a> {
    pub base_path: stdlib::DotPathBuf,
    ctx: &'a crate::ProcContext,
}
#[automatically_derived]
impl<'a> ::core::clone::Clone for ContractAddressWrapperNext<'a> {
    #[inline]
    fn clone(&self) -> ContractAddressWrapperNext<'a> {
        ContractAddressWrapperNext {
            base_path: ::core::clone::Clone::clone(&self.base_path),
            ctx: ::core::clone::Clone::clone(&self.ctx),
        }
    }
}
impl<'a> ContractAddressWrapperNext<'a> {
    pub fn new(ctx: &'a crate::ProcContext, base_path: stdlib::DotPathBuf) -> Self {
        Self { base_path, ctx }
    }
    pub fn name(&self) -> String {
        self.ctx.__get(self.base_path.push("name")).unwrap()
    }
    pub fn height(&self) -> i64 {
        self.ctx.__get(self.base_path.push("height")).unwrap()
    }
    pub fn tx_index(&self) -> i64 {
        self.ctx.__get(self.base_path.push("tx_index")).unwrap()
    }
    pub fn set_name(&self, value: String) {
        self.ctx.__set(self.base_path.push("name"), value);
    }
    pub fn set_height(&self, value: i64) {
        self.ctx.__set(self.base_path.push("height"), value);
    }
    pub fn set_tx_index(&self, value: i64) {
        self.ctx.__set(self.base_path.push("tx_index"), value);
    }
    pub fn load(&self) -> ContractAddress {
        ContractAddress {
            name: self.name(),
            height: self.height(),
            tx_index: self.tx_index(),
        }
    }
}
