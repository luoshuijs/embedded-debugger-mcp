---
name: embedded-debugger
description: Embedded hardware debugging workflow for probe-rs targets using embedded-debugger-mcp. Use when Codex or Claude Code needs to inspect debug probes, validate embedded debugger setup, start the MCP server, guide a user through ARM Cortex-M/RISC-V flashing/debugging/RTT workflows, or operate without installing an MCP client by using the CLI plus prompts.
---

# Embedded Debugger

Use the local `embedded-debugger-mcp` binary as the source of truth. Prefer CLI
checks first, then MCP tools when an MCP client is available.

## Entry Decision

1. If the user has an MCP client configured, start or verify the server:
   `embedded-debugger-mcp serve`
2. If the user wants no MCP install, use CLI-first mode:
   `embedded-debugger-mcp doctor`, `embedded-debugger-mcp probes list`, and
   `embedded-debugger-mcp skill print-prompt`.
3. If hardware access is required, confirm the probe and target are connected
   before destructive actions such as flash erase or program.

## CLI Workflow

Run these in order and report the exact outcome:

```bash
embedded-debugger-mcp doctor
embedded-debugger-mcp probes list
embedded-debugger-mcp config show
```

Use JSON for automation:

```bash
embedded-debugger-mcp doctor --json
embedded-debugger-mcp probes list --json
```

## Backends

One tool set runs over two interchangeable engines, chosen at `connect`:

- `backend: "probe-rs"` (default) — native probe-rs; supports flash and RTT.
- `backend: "openocd"` (experimental) — talks to an already-running `openocd`
  over its GDB port via `openocd_address` (default `127.0.0.1:3333`). Use for
  chips probe-rs does not cover well. Core/memory/control and `diagnose_fault`
  work here; flash and RTT are not yet available on this backend. Not yet
  hardware-validated — verify results before trusting them.

The AI uses the same tools regardless of backend; only `connect` differs.

## MCP Workflow

Use MCP tools for session-based operations:

1. `list_probes`
2. `connect` (add `backend: "openocd"` and `openocd_address` to use OpenOCD)
3. Read-only checks such as `probe_info`, `get_status`, and `read_memory`
4. On a crash or halt, call `diagnose_fault`: it reads the Cortex-M SCB fault
   registers (CFSR/HFSR/MMFAR/BFAR/SHCSR/CPUID) plus PC/SP/LR and returns a
   compact structured evidence bundle in one call. Halt the target first for
   meaningful values; reason over the set fault bits yourself. Then call
   `unwind_exception` with `elf_path` to map the crash to a source line
   (full DWARF backtrace on probe-rs; faulting PC/LR on OpenOCD).
5. Mutating operations only after the user confirms target, file path, and risk:
   `write_memory`, `flash_erase`, `flash_program`, `run_firmware` (probe-rs)
6. RTT operations after firmware is running: `rtt_attach`, `rtt_channels`,
   `rtt_read`, `rtt_write`, `rtt_detach` (probe-rs)
7. `disconnect`

## Safety Rules

- Treat flash erase, flash program, memory write, reset, run, and RTT write as
  mutating hardware operations.
- Prefer read-only discovery before mutation.
- Respect project configuration limits for file paths, file sizes, memory
  ranges, and flash erase permissions.
- Do not claim hardware success from command text alone; cite the command or MCP
  tool result that produced the evidence.

## Prompt Reference

For a reusable CLI+Skill prompt, read
`references/default-prompt.md`.
