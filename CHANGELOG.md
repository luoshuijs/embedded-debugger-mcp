# Changelog

## 0.3.0

Dual debug backend and AI-facing crash diagnosis.

### Added
- **Dual backend** behind a single `DebugBackend` trait, selected at `connect`:
  - `probe-rs` (default) — native probe-rs; full flash and RTT support.
  - `openocd` (experimental) — talks to an already-running `openocd` over the
    GDB Remote Serial Protocol (`openocd_address`, default `127.0.0.1:3333`).
    The AI uses the same tool set for both; only `connect` differs.
- `diagnose_fault` — reads the Cortex-M SCB fault registers
  (CFSR/HFSR/MMFAR/BFAR/SHCSR/CPUID) plus PC/SP/LR in one call and returns a
  compact structured evidence bundle with the set fault bits. Reports raw
  evidence; it does not assert a root cause.
- `unwind_exception` — maps a crash to source lines. On the probe-rs backend it
  returns a full DWARF backtrace via probe-rs's own unwinder; on the OpenOCD
  backend it reads the Cortex-M exception stack frame and maps the faulting
  PC/caller LR. Requires an `elf_path` built with debug info.
- 24 MCP tools total (added `diagnose_fault`, `unwind_exception`).

### Validated on real hardware
- OpenOCD backend verified end-to-end on a real **ESP32-S3** (openocd-esp32):
  connect, memory read/write, halt/run/step/reset, and register reads
  (PC/SP/LR) are correct.

### Notes / limitations
- OpenOCD register reads use `monitor reg <name>` (openocd resolves names per
  architecture), so PC/SP/LR work across ARM/Xtensa/RISC-V. Start openocd with
  `gdb_memory_map disable` or it may reject the GDB connection.
- `diagnose_fault` and `unwind_exception` are Cortex-M specific and do not apply
  to Xtensa (ESP32) targets. Their probe-rs paths delegate to probe-rs's tested
  DWARF/register code; they are unit-tested but not yet exercised on a physical
  Cortex-M crash.

## 0.2.0

- Initial MCP server for embedded debugging via probe-rs (probe discovery,
  target sessions, memory, breakpoints, flash, RTT), CLI, and bundled skill.
</content>
