use stdlib::WrapperNext;

#[derive(WrapperNext)]
enum Error {
    Message(String),
}
