# 嵌入式调试器 MCP 服务器

[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://rust-lang.org)
[![RMCP](https://img.shields.io/badge/RMCP-0.3.2-blue.svg)](https://github.com/modelcontextprotocol/rust-sdk)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

Embedded Debugger MCP 是一个基于 probe-rs 的 Rust 嵌入式调试服务器。它为
AI 助手提供 MCP 工具，同时也提供小型 CLI 和内置 skill，让用户即使不安装
MCP 客户端，也可以先用命令行工作流完成检查和引导。

语言版本: [English](README.md) | [中文](README_zh.md)

## 功能

- MCP 工具覆盖探针发现、目标连接、核心控制、内存访问、断点、Flash 编程和
  RTT 通信。
- CLI 命令覆盖环境检查、配置查看、探针列表、MCP 启动和 skill prompt 输出。
- 内置 Codex / Claude Code 兼容 skill: `skills/embedded-debugger`。
- 发布检查覆盖 rustfmt、clippy、测试、文档、打包和 STM32 demo 构建。

## 架构

```text
MCP client or CLI
        |
        v
embedded-debugger-mcp
        |
        v
probe-rs -> debug probe -> target MCU
```

## 要求

- Rust stable 工具链。
- probe-rs 兼容调试探针，例如 ST-Link、J-Link、DAPLink、Black Magic Probe
  或受支持的 FTDI 探针。
- 目标芯片和可工作的 SWD/JTAG 连线。
- STM32 demo 固件检查需要 nightly Rust 和 `rust-src`。

## 构建

```bash
git clone https://github.com/adancurusul/embedded-debugger-mcp.git
cd embedded-debugger-mcp
cargo build --release
```

二进制位于 `target/release/embedded-debugger-mcp`。

## MCP 模式

显式启动服务器:

```bash
embedded-debugger-mcp serve
```

为了兼容旧配置，不带子命令运行 `embedded-debugger-mcp` 也会通过 stdio
启动 MCP 服务。

MCP 客户端配置示例:

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

Windows 下请使用 `.exe` 路径和 Windows 路径分隔符。

## CLI 与 Skill 模式

CLI 模式适合在 MCP 客户端配置前做环境检查、自动化和 agent 工作流。

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

内置 skill 位于:

```text
skills/embedded-debugger/
```

它是普通 Codex skill，同时通过 `.claude-plugin/plugin.json` 提供 Claude Code
插件加载方式。工作流会先运行 CLI 检查；只有 MCP 客户端可用时，才会使用 MCP
工具做会话型调试操作。

安装到 Codex 和 Claude Code:

```bash
embedded-debugger-mcp skill install --target both
```

可以先预览，也可以用 JSON 做自动化:

```bash
embedded-debugger-mcp skill install --target both --dry-run --json
embedded-debugger-mcp skill install --target both --force --json
```

`--target` 支持 `codex`、`claude` 或 `both`。该命令会把 Codex skill 安装到
`~/.codex/skills/embedded-debugger`，把 Claude Code skill 安装到
`~/.claude/skills/embedded-debugger`，并把可通过 `--plugin-dir` 加载的 Claude
plugin 包安装到 `~/.claude/plugins/local/embedded-debugger-mcp`。已有目录默认不会
覆盖，需要显式传入 `--force`。

Codex 里可以用类似这样的 prompt 触发:

```text
Use $embedded-debugger to inspect my embedded target setup.
```

从已安装的 Claude Code plugin 包加载:

```bash
claude --plugin-dir ~/.claude/plugins/local/embedded-debugger-mcp --print '/embedded-debugger inspect my embedded target setup'
```

对于只支持 skill 目录的环境，也可以直接复制 `skills/embedded-debugger` 到本地
skills 目录。

验证 skill 包:

```bash
python3 .github/scripts/validate_skill.py skills/embedded-debugger
python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/embedded-debugger
claude plugin validate .
embedded-debugger-mcp skill install --target both --home /tmp/embedded-debugger-skill-home --force --json
```

第一个命令验证仓库内 skill 元数据，第二个命令在已安装 Codex skill creator
validator 时验证标准 `SKILL.md` 布局，第三个命令验证 Claude Code 插件 manifest。

## 后端

同一套工具运行在两个可互换的引擎之上，在 `connect` 时选择：

- `backend: "probe-rs"`（默认）——原生 probe-rs，完整支持 flash 与 RTT。
- `backend: "openocd"`（实验性）——通过 GDB Remote Serial Protocol 连接一个已运行
  的 `openocd`（`openocd_address`，默认 `127.0.0.1:3333`），用于 probe-rs 覆盖不佳的
  芯片。核心/内存/控制/断点与 `diagnose_fault` 可用；flash 与 RTT 暂仅 probe-rs 支持。
  该后端尚未经过真实硬件验证。

对 AI 而言两者是同一套工具，只有 `connect` 参数不同。

## MCP 工具集

探针管理:

| 工具 | 用途 |
|------|------|
| `list_probes` | 发现连接的调试探针。 |
| `connect` | 为目标芯片打开探针会话。 |
| `probe_info` | 查看活动会话信息。 |

目标控制:

| 工具 | 用途 |
|------|------|
| `halt` | 暂停核心执行。 |
| `run` | 恢复执行。 |
| `reset` | 复位核心。当前服务器只接受已实现的硬件风格复位路径。 |
| `step` | 单步执行一条指令。 |
| `get_status` | 读取核心和会话状态。 |
| `disconnect` | 断开会话并清理资源。 |

内存与断点:

| 工具 | 用途 |
|------|------|
| `read_memory` | 在配置的大小和范围限制内读取目标内存。 |
| `write_memory` | 在配置允许时写入目标内存。 |
| `set_breakpoint` | 设置硬件断点。 |
| `clear_breakpoint` | 清除硬件断点。 |

诊断:

| 工具 | 用途 |
|------|------|
| `diagnose_fault` | 一次调用读取 Cortex-M SCB 故障寄存器（CFSR/HFSR/MMFAR/BFAR/SHCSR/CPUID）及 PC/SP/LR，返回含已置位故障标志的紧凑结构化证据。只给原始证据、不下根因结论。请先 halt 目标。 |
| `unwind_exception` | 崩溃后回溯栈并把每帧映射到源码行。probe-rs 后端用 probe-rs 自带回溯器给出完整 DWARF 调用栈（每帧函数名+file:line）；OpenOCD 后端读取 Cortex-M 异常栈帧，把出错 PC/调用者 LR 映射到源码行。需 `elf_path` 指向带调试信息(`.debug_line`)的固件。 |

Flash:

| 工具 | 用途 |
|------|------|
| `flash_erase` | 在擦除权限开启时擦除 Flash。 |
| `flash_program` | 通过 probe-rs Flash 算法烧录 ELF、HEX 或 BIN。 |
| `flash_verify` | 将原始期望数据与目标 Flash 内容比较。 |
| `run_firmware` | 擦除、烧录、复位/运行，并可选择连接 RTT。 |

RTT:

| 工具 | 用途 |
|------|------|
| `rtt_attach` | 连接 SEGGER RTT 控制块。 |
| `rtt_detach` | 断开 RTT。 |
| `rtt_channels` | 列出发现的 RTT 通道。 |
| `rtt_read` | 从上行通道读取，并遵守最大字节数和超时限制。 |
| `rtt_write` | 向下行通道写入。 |

## 安全说明

- 除非启用 `security.allow_flash_erase` 或 `flash.allow_erase`，否则 Flash
  擦除会被拒绝。
- 内存写入受 `security.allow_memory_write` 控制。
- 可选内存范围限制使用配置中的目标 memory region。
- 固件文件路径会做 canonicalize；如果配置了 `security.allowed_file_paths`，
  还会检查路径是否在允许目录内，并检查文件大小限制。
- 任意地址的 sector erase 目前会被拒绝，直到实现目标相关的 sector 映射。
- `flash_verify` 做原始数据比较。文件方式验证请使用原始 BIN 文件或十六进制数据。

生成起始配置:

```bash
embedded-debugger-mcp config generate > embedded-debugger.toml
embedded-debugger-mcp --config embedded-debugger.toml config validate
```

## STM32 Demo

STM32 RTT demo 位于 `examples/STM32_demo`。

```bash
cd examples/STM32_demo
CARGO_TARGET_DIR=/tmp/embedded-debugger-mcp-stm32-target cargo +nightly check --locked
```

该 demo 展示多通道 RTT 通信，主要用于硬件验证。详见
[examples/STM32_demo/README.md](examples/STM32_demo/README.md)。

## 发布检查

发布前运行:

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

## 致谢

- [probe-rs](https://probe.rs/) 提供嵌入式调试探针支持。
- [rmcp](https://github.com/modelcontextprotocol/rust-sdk) 提供 Rust MCP SDK。
- [tokio](https://tokio.rs/) 提供异步运行时。

## 许可证

本项目采用 MIT 许可证。详见 [LICENSE](LICENSE)。
