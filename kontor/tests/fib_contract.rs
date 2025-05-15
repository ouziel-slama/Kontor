use std::path::Path;

use anyhow::anyhow;
use kontor::logging;
use tokio::fs::read;
use wasmtime::{
    Engine, Store,
    component::{Component, Linker, Type, Val, wasm_wave::parser::Parser as WaveParser},
};
use wit_parser::{WorldItem, WorldKey};

#[tokio::test]
async fn test_fib_contract() -> Result<(), Box<dyn std::error::Error>> {
    logging::setup();

    let path = Path::new("../target/wasm32-unknown-unknown/debug/fib.wasm");
    let wasm = read(path).await?;
    assert!(wasmparser::Parser::is_component(&wasm));

    let n = 8;
    let s = format!("fib({})", n);
    let call = WaveParser::new(s.as_str()).parse_raw_func_call()?;
    let wit = wit_component::decode(&wasm)?;
    let world = wit
        .resolve()
        .worlds
        .iter()
        .next()
        .ok_or(anyhow!("world not found"))?;
    let k = WorldKey::Name(call.name().to_string());
    let v = world
        .1
        .exports
        .get(&k)
        .ok_or(anyhow!("function not found in WIT"))?;
    let WorldItem::Function(signature) = v else {
        panic!("failed");
    };
    let param_types = signature
        .params
        .iter()
        .map(|p| match p.1 {
            wit_parser::Type::U64 => Type::U64,
            _ => unimplemented!(),
        })
        .collect::<Vec<_>>();
    let mut results = signature
        .result
        .as_slice()
        .iter()
        .map(|t| match t {
            wit_parser::Type::U64 => Val::U64(0),
            _ => unimplemented!(),
        })
        .collect::<Vec<_>>();

    let mut config = wasmtime::Config::new();
    config.async_support(true);
    let engine = Engine::new(&config)?;
    let component = Component::from_binary(&engine, &wasm)?;
    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let instance = linker.instantiate_async(&mut store, &component).await?;
    let f = instance
        .get_func(&mut store, call.name())
        .ok_or(anyhow!("can't find fib"))?;
    let params: Vec<Val> = call.to_wasm_params(&param_types)?;
    f.call_async(&mut store, &params, &mut results).await?;
    assert_eq!(results[0], Val::U64(21));
    Ok(())
}
