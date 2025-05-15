use std::path::Path;

use anyhow::anyhow;
use kontor::logging;
use wasmtime::{
    Engine, Store,
    component::{Component, Linker, Type, Val, wasm_wave::parser::Parser},
};

#[tokio::test]
async fn test_fib_contract() -> Result<(), Box<dyn std::error::Error>> {
    logging::setup();
    let path = Path::new("../target/wasm32-unknown-unknown/debug/fib.wasm");
    let mut config = wasmtime::Config::new();
    config.async_support(true);
    let engine = Engine::new(&config)?;
    let component = Component::from_file(&engine, path)?;
    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let instance = linker.instantiate_async(&mut store, &component).await?;
    let n = 8;
    let s = format!("fib({})", n);
    let call = Parser::new(s.as_str()).parse_raw_func_call()?;
    let f = instance
        .get_func(&mut store, call.name())
        .ok_or(anyhow!("can't find fib"))?;
    let param_types = [Type::U64];
    let params: Vec<Val> = call.to_wasm_params(&param_types)?;
    let mut results = [Val::U64(0)];
    f.call_async(&mut store, &params, &mut results).await?;
    assert_eq!(results[0], Val::U64(21));
    Ok(())
}
