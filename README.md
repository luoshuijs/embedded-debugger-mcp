# Embedded Debugger MCP Server

[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://rust-lang.org)
[![RMCP](https://img.shields.io/badge/RMCP-0.3.2-blue.svg)](https://github.com/modelcontextprotocol/rust-sdk)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

Embedded Debugger MCP is a Rust server for embedded debugging through
probe-rs. It exposes MCP tools for AI assistants and also includes a small CLI
and bundled skill for users who want a command-driven workflow without setting
up an MCP client first.

Language versions: [English](README.md) | [中文](README_zh.md)

## What It Provides

- MCP tools for probe discovery, target connection, core control, memory access,
  breakpoints, flash programming, and RTT communication.
- CLI commands for environment checks, configuration inspection, probe listing,
  MCP serving, and skill prompt handoff.
- A Codex/Claude Code compatible skill at `skills/embedded-debugger`.
- Release checks covering rustfmt, clippy, tests, docs, packaging, and the STM32
  demo build.

## Architecture

```text
MCP client or CLI
        |
        v
embedded-debugger-mcp
        |
        v
probe-rs -> debug probe -> target MCU
```

## Requirements

- Rust stable toolchain.
- A probe-rs compatible debug probe such as ST-Link, J-Link, DAPLink, Black
  Magic Probe, or a supported FTDI-based probe.
- A supported target chip and working SWD/JTAG wiring for hardware operations.
- Nightly Rust plus `rust-src` for the bundled STM32 demo firmware check.

## Build

```bash
git clone https://github.com/adancurusul/embedded-debugger-mcp.git
cd embedded-debugger-mcp
cargo build --release
```

The binary is `target/release/embedded-debugger-mcp`.

## MCP Mode

Run the server explicitly:

```bash
embedded-debugger-mcp serve
```

For compatibility, running `embedded-debugger-mcp` without a subcommand also
serves MCP over stdio.

Example MCP client configuration:

```json
{
  "mcpServers": {
    "embedded-debugger": {
      "command": "/path/to/embedded-debugger-mcp/target/release/embedded-debugger-mcp",
      "args": ["serve"],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

On Windows, use the `.exe` path and Windows path separators.

## CLI And Skill Mode

CLI mode is useful for setup checks, automation, and agent workflows before an
MCP client is configured.

```bash
embedded-debugger-mcp doctor
embedded-debugger-mcp doctor --json
embedded-debugger-mcp probes list
embedded-debugger-mcp probes list --json
embedded-debugger-mcp config generate
embedded-debugger-mcp config validate
embedded-debugger-mcp config show
embedded-debugger-mcp skill print-prompt
embedded-debugger-mcp skill install --target both
```

The bundled skill lives in:

```text
skills/embedded-debugger/
```

It is written as a plain Codex skill and is also packaged for Claude Code with
`.claude-plugin/plugin.json`. The skill starts with CLI checks and uses MCP
tools only when an MCP client is available.

Install the skill for Codex and Claude Code:

```bash
embedded-debugger-mcp skill install --target both
```

Preview or automate the install with JSON:

```bash
embedded-debugger-mcp skill install --target both --dry-run --json
embedded-debugger-mcp skill install --target both --force --json
```

`--target` accepts `codex`, `claude`, or `both`. The command installs the Codex
skill under `~/.codex/skills/embedded-debugger`, the Claude Code skill under
`~/.claude/skills/embedded-debugger`, and a local Claude plugin-dir package under
`~/.claude/plugins/local/embedded-debugger-mcp`. Existing directories are not
replaced unless `--force` is passed.

Trigger the Codex skill with a prompt such as:

```text
Use $embedded-debugger to inspect my embedded target setup.
```

Load the installed Claude Code plugin package:

```bash
claude --plugin-dir ~/.claude/plugins/local/embedded-debugger-mcp --print '/embedded-debugger inspect my embedded target setup'
```

For skill-only environments, the same `skills/embedded-debugger` directory can
also be copied to a local skills directory.

Validate the skill package:

```bash
python3 .github/scripts/validate_skill.py skills/embedded-debugger
python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/embedded-debugger
claude plugin validate .
embedded-debugger-mcp skill install --target both --home /tmp/embedded-debugger-skill-home --force --json
```

The first command validates the repository skill metadata, the second validates
the standard `SKILL.md` layout when the Codex skill creator validator is
installed, and the third validates the Claude Code plugin manifest.

## Backends

The tools run over one of two interchangeable engines, selected at `connect`:

- `backend: "probe-rs"` (default) — native probe-rs; full flash and RTT support.
- `backend: "openocd"` (experimental) — connects to an already-running `openocd`
  over its GDB port (`openocd_address`, default `127.0.0.1:3333`) using the GDB
  Remote Serial Protocol. Intended for targets probe-rs does not cover. Core,
  memory, control, breakpoints, and `diagnose_fault` are available; flash and RTT
  are probe-rs only for now. This backend is not yet hardware-validated.

The AI sees the same tool set for both; only `connect` arguments change.

## MCP Tool Set

Probe management:

| Tool | Purpose |
|------|---------|
| `list_probes` | Discover connected debug probes. |
| `connect` | Open a probe session for a target chip. |
| `probe_info` | Show active session information. |

Target control:

| Tool | Purpose |
|------|---------|
| `halt` | Halt core execution. |
| `run` | Resume execution. |
| `reset` | Reset the core. Only the implemented hardware-style reset path is accepted. |
| `step` | Single-step one instruction. |
| `get_status` | Read core/session status. |
| `disconnect` | Drop the session and clean up resources. |

Memory and breakpoints:

| Tool | Purpose |
|------|---------|
| `read_memory` | Read target memory with configured size/range limits. |
| `write_memory` | Write target memory when enabled by configuration. |
| `set_breakpoint` | Set a hardware breakpoint. |
| `clear_breakpoint` | Clear a hardware breakpoint. |

Diagnostics:

| Tool | Purpose |
|------|---------|
| `diagnose_fault` | Read the Cortex-M SCB fault registers (CFSR/HFSR/MMFAR/BFAR/SHCSR/CPUID) plus PC/SP/LR in one call and return a compact structured evidence bundle with the set fault bits. Reports raw evidence; it does not assert a root cause. Halt the target first. |
| `unwind_exception` | Unwind the stack after a crash and map each frame to a source line. On the probe-rs backend, returns a full DWARF backtrace (function + file:line per frame) via probe-rs's own unwinder; on the OpenOCD backend, reads the Cortex-M exception stack frame and maps the faulting PC / caller LR to source lines. Needs `elf_path` to firmware built with debug info (`.debug_line`). |

Flash:

| Tool | Purpose |
|------|---------|
| `flash_erase` | Erase flash when erase permissions are enabled. |
| `flash_program` | Program ELF, HEX, or BIN files through probe-rs flash algorithms. |
| `flash_verify` | Compare raw expected data with target flash contents. |
| `run_firmware` | Erase, program, reset/run, and optionally attach RTT. |

RTT:

| Tool | Purpose |
|------|---------|
| `rtt_attach` | Attach to a SEGGER RTT control block. |
| `rtt_detach` | Detach RTT. |
| `rtt_channels` | List discovered RTT channels. |
| `rtt_read` | Read from an up channel with max byte and timeout limits. |
| `rtt_write` | Write to a down channel. |

## Safety Notes

- Flash erase is disabled unless `security.allow_flash_erase` or
  `flash.allow_erase` is enabled.
- Memory writes are controlled by `security.allow_memory_write`.
- Optional memory range restriction uses target memory regions in the
  configuration.
- Firmware file paths are canonicalized, checked against
  `security.allowed_file_paths` when configured, and checked against size
  limits.
- Sector erase by arbitrary address is rejected until target-specific sector
  mapping is implemented.
- `flash_verify` supports raw data comparison. Use raw BIN files or hex data for
  file-based verification.

Generate a starting configuration:

```bash
embedded-debugger-mcp config generate > embedded-debugger.toml
embedded-debugger-mcp --config embedded-debugger.toml config validate
```

## STM32 Demo

The STM32 RTT demo is in `examples/STM32_demo`.

```bash
cd examples/STM32_demo
CARGO_TARGET_DIR=/tmp/embedded-debugger-mcp-stm32-target cargo +nightly check --locked
```

The demo firmware shows multi-channel RTT communication and is intended as a
hardware validation aid. See [examples/STM32_demo/README.md](examples/STM32_demo/README.md).

## Release Checks

Run these before cutting a release:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --all-features --no-deps
cargo package --locked
python3 .github/scripts/validate_skill.py skills/embedded-debugger
claude plugin validate .
embedded-debugger-mcp skill install --target both --home /tmp/embedded-debugger-skill-home --force --json
(cd examples/STM32_demo && CARGO_TARGET_DIR=/tmp/embedded-debugger-mcp-stm32-target cargo +nightly check --locked)
```

## Acknowledgments

- [probe-rs](https://probe.rs/) for embedded debug probe support.
- [rmcp](https://github.com/modelcontextprotocol/rust-sdk) for the Rust MCP SDK.
- [tokio](https://tokio.rs/) for the async runtime.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
