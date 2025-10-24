use stdlib::Store;
enum Error {
    Message(String),
    Overflow,
}
#[automatically_derived]
impl stdlib::Store for Error {
    fn __set(
        ctx: &impl stdlib::WriteContext,
        base_path: stdlib::DotPathBuf,
        value: Error,
    ) {
        ctx.__delete_matching_paths(&base_path, &["message", "overflow"]);
        match value {
            Error::Message(inner) => ctx.__set(base_path.push("message"), inner),
            Error::Overflow => ctx.__set(base_path.push("overflow"), ()),
        }
    }
}
