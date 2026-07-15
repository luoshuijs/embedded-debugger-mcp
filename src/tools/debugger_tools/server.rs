use rmcp::{
    model::*, service::RequestContext, tool_handler, ErrorData as McpError, RoleServer,
    ServerHandler,
};
use tracing::info;

use super::session::EmbeddedDebuggerToolHandler;

#[tool_handler]
impl ServerHandler for EmbeddedDebuggerToolHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Embedded debugging and flash programming MCP server for ARM Cortex-M, RISC-V, and other targets. A single tool set runs over two interchangeable backends chosen at connect: probe-rs (default, native, RTT/flash) or OpenOCD (backend=\"openocd\", via GDB RSP, for chips probe-rs does not cover). Exposes 23 tools: list_probes, connect, disconnect, probe_info, halt, run, reset, step, get_status, read_memory, write_memory, set_breakpoint, clear_breakpoint, diagnose_fault, rtt_attach, rtt_detach, rtt_read, rtt_write, rtt_channels, flash_erase, flash_program, flash_verify, run_firmware.".to_string()),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        info!("Embedded Debugger MCP server initialized with 23 tools (dual backend: probe-rs + OpenOCD)");
        Ok(self.get_info())
    }
}
