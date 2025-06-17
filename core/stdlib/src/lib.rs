wasmtime::component::bindgen!({
    path: "wit/stdlib.wit",
    with: {
        "kontor:contract/stdlib/monoid": MyMonoidHostRep,
        "kontor:contract/stdlib/foreign": ForeignHostRep,
    },
    async: true,
    trappable_imports: true,
});

mod monoid;
mod foreign;

pub use kontor::contract::stdlib::*;
pub use monoid::MyMonoidHostRep;
pub use foreign::ForeignHostRep;
