use stdlib::WrapperNext;
enum Error {
    Message(String),
}
pub enum ErrorWrapperNext {
    Message(String),
}
#[automatically_derived]
impl ::core::clone::Clone for ErrorWrapperNext {
    #[inline]
    fn clone(&self) -> ErrorWrapperNext {
        match self {
            ErrorWrapperNext::Message(__self_0) => {
                ErrorWrapperNext::Message(::core::clone::Clone::clone(__self_0))
            }
        }
    }
}
impl ErrorWrapperNext {
    pub fn new(ctx: &crate::ProcContext, base_path: stdlib::DotPathBuf) -> Self {
        ctx.__extend_path_with_match(&base_path, &["message"])
            .map(|path| match path {
                p if p.starts_with(base_path.push("message").as_ref()) => {
                    ErrorWrapperNext::Message(
                        ctx.__get(base_path.push("message")).unwrap(),
                    )
                }
                _ => {
                    ::core::panicking::panic_fmt(
                        format_args!("Matching path not found"),
                    );
                }
            })
            .unwrap()
    }
    pub fn load(&self) -> Error {
        match self {
            ErrorWrapperNext::Message(inner) => Error::Message(inner.clone()),
        }
    }
}
