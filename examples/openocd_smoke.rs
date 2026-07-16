//! Real-hardware smoke test for the OpenOCD backend.
//!
//! Drives `OpenOcdBackend` against a running `openocd` GDB server (default
//! 127.0.0.1:3333) and prints each operation's result. Intended to be run
//! against real hardware (e.g. ESP32-S3 via openocd-esp32) to confirm the RSP
//! protocol wiring end-to-end.
//!
//! Usage:
//!   cargo run --release --example openocd_smoke -- [addr] [mem_hex]
//!
//! Notes: memory read/write and halt/run are architecture-neutral. Registers
//! are read by name via openocd's `monitor reg`, so PC/SP/LR read correctly on
//! both ARM and Xtensa (verified on real ESP32-S3).

use embedded_debugger_mcp::backend::{CoreRegId, DebugBackend, OpenOcdBackend};

#[tokio::main]
async fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:3333".to_string());
    // Default read address: ESP32-S3 internal SRAM1.
    let mem_addr = std::env::args()
        .nth(2)
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x3FC8_8000);

    println!("== openocd smoke test ==");
    println!("connecting to {addr} ...");
    let mut be = match OpenOcdBackend::connect(&addr).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("CONNECT FAILED: {e}");
            std::process::exit(1);
        }
    };
    println!("connected (backend = {})", be.kind());

    match be.halt().await {
        Ok(()) => println!("halt        : OK"),
        Err(e) => println!("halt        : ERR {e}"),
    }
    match be.status().await {
        Ok(s) => println!("status      : {s}"),
        Err(e) => println!("status      : ERR {e}"),
    }

    for a in [mem_addr, 0x4000_0000] {
        match be.read_bytes(a, 16).await {
            Ok(d) => {
                let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
                println!("read 0x{a:08X}: {hex}");
            }
            Err(e) => println!("read 0x{a:08X}: ERR {e}"),
        }
    }

    // Read by name via 'monitor reg' — correct on ARM and Xtensa.
    for reg in [CoreRegId::Pc, CoreRegId::Sp, CoreRegId::Lr] {
        match be.core_reg(reg).await {
            Ok(v) => println!("{reg:<10?}: 0x{v:08X}"),
            Err(e) => println!("{reg:<10?}: ERR {e}"),
        }
    }

    match be.run().await {
        Ok(()) => println!("run         : OK"),
        Err(e) => println!("run         : ERR {e}"),
    }
    println!("== done ==");
}
