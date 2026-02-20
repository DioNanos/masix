use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::sync::mpsc;
use std::time::Duration;
use wasmi::{
    Config, Engine, Extern, ExternType, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

pub const DEFAULT_TIMEOUT_MS: u64 = 1500;
pub const HARD_MAX_TIMEOUT_MS: u64 = 5000;
pub const DEFAULT_MAX_FUEL: u64 = 5_000_000;
pub const HARD_MAX_FUEL: u64 = 50_000_000;
pub const DEFAULT_MAX_MEMORY_BYTES: usize = 8 * 1024 * 1024;
pub const HARD_MAX_MEMORY_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;
pub const HARD_MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct WasmLimits {
    pub timeout_ms: u64,
    pub max_fuel: u64,
    pub max_memory_bytes: usize,
    pub max_output_bytes: usize,
}

impl Default for WasmLimits {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_fuel: DEFAULT_MAX_FUEL,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        }
    }
}

fn clamp_limits(mut lim: WasmLimits) -> WasmLimits {
    if lim.timeout_ms == 0 {
        lim.timeout_ms = DEFAULT_TIMEOUT_MS;
    }
    lim.timeout_ms = lim.timeout_ms.min(HARD_MAX_TIMEOUT_MS);
    if lim.max_fuel == 0 {
        lim.max_fuel = DEFAULT_MAX_FUEL;
    }
    lim.max_fuel = lim.max_fuel.min(HARD_MAX_FUEL);
    if lim.max_memory_bytes == 0 {
        lim.max_memory_bytes = DEFAULT_MAX_MEMORY_BYTES;
    }
    lim.max_memory_bytes = lim.max_memory_bytes.min(HARD_MAX_MEMORY_BYTES);
    if lim.max_output_bytes == 0 {
        lim.max_output_bytes = DEFAULT_MAX_OUTPUT_BYTES;
    }
    lim.max_output_bytes = lim.max_output_bytes.min(HARD_MAX_OUTPUT_BYTES);
    lim
}

#[derive(Debug, Clone)]
pub struct WasmAbi {
    pub main: String,
    pub out_ptr: String,
    pub memory: String,
}

impl Default for WasmAbi {
    fn default() -> Self {
        Self {
            main: "masix_main".to_string(),
            out_ptr: "masix_out_ptr".to_string(),
            memory: "memory".to_string(),
        }
    }
}

pub fn validate_module_exports(wasm: &[u8], abi: &WasmAbi) -> Result<()> {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm).context("invalid wasm module")?;

    let mut has_main = false;
    let mut has_out_ptr = false;
    let mut has_memory = false;

    for export in module.exports() {
        match export.ty() {
            ExternType::Func(_) => {
                if export.name() == abi.main {
                    has_main = true;
                } else if export.name() == abi.out_ptr {
                    has_out_ptr = true;
                }
            }
            ExternType::Memory(_) => {
                if export.name() == abi.memory {
                    has_memory = true;
                }
            }
            _ => {}
        }
    }

    if !has_main {
        bail!("missing required export: {}", abi.main);
    }
    if !has_out_ptr {
        bail!("missing required export: {}", abi.out_ptr);
    }
    if !has_memory {
        bail!("missing required export: {}", abi.memory);
    }
    Ok(())
}

pub fn run_wasm_tool(
    wasm: Vec<u8>,
    input: Value,
    abi: WasmAbi,
    limits: WasmLimits,
) -> Result<String> {
    let limits = clamp_limits(limits);
    let timeout_ms = limits.timeout_ms;

    // Input must be a JSON object.
    if !input.is_object() {
        bail!("input must be a JSON object");
    }
    let input_bytes = serde_json::to_vec(&input).context("serialize input json failed")?;
    if input_bytes.len() > 64 * 1024 {
        bail!("input too large (max 64KiB)");
    }

    let (tx, rx) = mpsc::channel();
    let limits_for_thread = limits.clone();
    std::thread::spawn(move || {
        let res = run_inner(&wasm, &input_bytes, &abi, &limits_for_thread);
        let _ = tx.send(res);
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(res) => res,
        Err(mpsc::RecvTimeoutError::Timeout) => bail!("wasm timeout"),
        Err(_) => bail!("wasm runner failed"),
    }
}

fn run_inner(wasm: &[u8], input: &[u8], abi: &WasmAbi, limits: &WasmLimits) -> Result<String> {
    let mut cfg = Config::default();
    cfg.consume_fuel(true);
    let engine = Engine::new(&cfg);

    let module = Module::new(&engine, wasm).context("invalid wasm module")?;

    // No host imports; no WASI.
    let linker = Linker::new(&engine);

    // Store limits to cap memory growth.
    let store_limits = StoreLimitsBuilder::new()
        .memory_size(limits.max_memory_bytes)
        .build();

    let mut store = Store::new(&engine, store_limits);
    // Apply memory growth limiter.
    store.limiter(|lim: &mut StoreLimits| lim);
    // Stores start with no fuel; without this, infinite loops will not stop.
    store.set_fuel(limits.max_fuel).context("set fuel failed")?;

    let instance = linker
        .instantiate(&mut store, &module)
        .context("instantiate failed")?
        .start(&mut store)
        .context("start failed")?;

    let memory = resolve_memory(&instance, &mut store, &abi.memory)?;
    let main: TypedFunc<(i32, i32), i32> = instance
        .get_typed_func(&mut store, &abi.main)
        .context("missing main export")?;
    let out_ptr: TypedFunc<(), i32> = instance
        .get_typed_func(&mut store, &abi.out_ptr)
        .context("missing out_ptr export")?;

    let mem_size = memory.data_size(&store);
    if input.len() > mem_size {
        bail!("wasm memory too small for input");
    }

    // ABI: host writes input JSON at offset 0 and calls masix_main(0, len).
    memory
        .write(&mut store, 0, input)
        .context("write input failed")?;

    let out_len = main
        .call(&mut store, (0_i32, input.len() as i32))
        .context("guest trap in masix_main")?;
    if out_len < 0 {
        bail!("guest returned negative out_len");
    }
    let out_len = out_len as usize;
    if out_len > limits.max_output_bytes {
        bail!("tool output exceeds limit");
    }
    let out_ptr = out_ptr
        .call(&mut store, ())
        .context("guest trap in out_ptr")?;
    if out_ptr < 0 {
        bail!("guest returned negative out_ptr");
    }
    let out_ptr = out_ptr as usize;

    if out_ptr + out_len > memory.data_size(&store) {
        bail!("output out of bounds");
    }

    // Prevent overlap to avoid confusing self-modifying I/O.
    if out_ptr < input.len() {
        bail!("output overlaps input");
    }

    let mut out = vec![0_u8; out_len];
    memory
        .read(&store, out_ptr, &mut out)
        .context("read output failed")?;

    let out_str = String::from_utf8(out).context("tool output must be utf-8")?;
    Ok(out_str)
}

fn resolve_memory(
    instance: &Instance,
    store: &mut Store<StoreLimits>,
    name: &str,
) -> Result<Memory> {
    match instance.get_export(store, name) {
        Some(Extern::Memory(mem)) => Ok(mem),
        _ => bail!("missing memory export: {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_echo_tool() -> Vec<u8> {
        let wat = r#"
        (module
          (memory (export "memory") 1)
          (data (i32.const 1024) "{\"ok\":true}")
          (func (export "masix_out_ptr") (result i32)
            i32.const 1024)
          (func (export "masix_main") (param i32 i32) (result i32)
            i32.const 11)
        )
        "#;
        wat::parse_str(wat).expect("wat")
    }

    fn wasm_infinite_loop() -> Vec<u8> {
        let wat = r#"
        (module
          (memory (export "memory") 1)
          (func (export "masix_out_ptr") (result i32) (i32.const 1024))
          (func (export "masix_main") (param i32 i32) (result i32)
            (loop $l
              br $l
            )
            i32.const 0)
        )
        "#;
        wat::parse_str(wat).expect("wat")
    }

    #[test]
    fn validate_exports_ok() {
        validate_module_exports(&wasm_echo_tool(), &WasmAbi::default()).unwrap();
    }

    #[test]
    fn run_returns_output() {
        let out = run_wasm_tool(
            wasm_echo_tool(),
            serde_json::json!({"x": 1}),
            WasmAbi::default(),
            WasmLimits {
                timeout_ms: 1000,
                max_fuel: 5_000_000,
                max_memory_bytes: 8 * 1024 * 1024,
                max_output_bytes: 64 * 1024,
            },
        )
        .unwrap();
        assert_eq!(out, "{\"ok\":true}");
    }

    #[test]
    fn fuel_limits_stop_infinite_loop() {
        let err = run_wasm_tool(
            wasm_infinite_loop(),
            serde_json::json!({}),
            WasmAbi::default(),
            WasmLimits {
                timeout_ms: 1000,
                max_fuel: 10_000,
                max_memory_bytes: 8 * 1024 * 1024,
                max_output_bytes: 64 * 1024,
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().to_ascii_lowercase().contains("trap")
                || err.to_string().to_ascii_lowercase().contains("fuel")
        );
    }
}
