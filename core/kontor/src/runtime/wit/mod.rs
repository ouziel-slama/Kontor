wasmtime::component::bindgen!({
    world: "contract",
    path: "src/runtime/wit",
    async: true,
    trappable_imports: true,
});
