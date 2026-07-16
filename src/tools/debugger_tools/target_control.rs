use rmcp::{handler::server::tool::Parameters, model::*, tool, tool_router, ErrorData as McpError};
use std::future::Future;
use tracing::{debug, error, info};

use super::session::EmbeddedDebuggerToolHandler;
use crate::backend::CoreState;
use crate::tools::types::*;

#[tool_router(router = target_control_tool_router, vis = "pub")]
impl EmbeddedDebuggerToolHandler {
    // =============================================================================
    // Target Control Tools (5 tools) — routed through the unified DebugBackend
    // =============================================================================

    #[tool(description = "Halt the target CPU execution")]
    async fn halt(
        &self,
        Parameters(args): Parameters<HaltArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Halting target for session: {}", args.session_id);
        let session_arc = self.get_session(&args.session_id).await?;

        let mut backend = session_arc.backend.lock().await;
        backend.halt().await.map_err(|e| {
            error!(
                "Failed to halt target for session {}: {}",
                args.session_id, e
            );
            McpError::internal_error(format!("Failed to halt target: {}", e), None)
        })?;
        let snap = backend.snapshot().await.ok();
        drop(backend);

        let (pc, sp) = snap.map(|s| (s.pc, s.sp)).unwrap_or((0, 0));
        let message = format!(
            "Target halted successfully!\n\n\
            Session ID: {}\n\
            PC: 0x{:08X}\n\
            SP: 0x{:08X}\n\
            State: Halted\n",
            args.session_id, pc, sp
        );
        info!("Halt completed for session: {}", args.session_id);
        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    #[tool(description = "Resume target CPU execution")]
    async fn run(&self, Parameters(args): Parameters<RunArgs>) -> Result<CallToolResult, McpError> {
        debug!("Running target for session: {}", args.session_id);
        let session_arc = self.get_session(&args.session_id).await?;

        {
            let mut backend = session_arc.backend.lock().await;
            backend.run().await.map_err(|e| {
                error!(
                    "Failed to run target for session {}: {}",
                    args.session_id, e
                );
                McpError::internal_error(format!("Failed to run target: {}", e), None)
            })?;
        }

        let message = format!(
            "Target resumed execution successfully!\n\n\
            Session ID: {}\n\
            Status: Running\n\n\
            The target is now executing code. Use 'halt' to stop execution.",
            args.session_id
        );
        info!("Run completed for session: {}", args.session_id);
        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    #[tool(description = "Reset the target CPU")]
    async fn reset(
        &self,
        Parameters(args): Parameters<ResetArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Resetting target for session: {}", args.session_id);

        if args.reset_type != "hardware" {
            return Err(McpError::internal_error(
                format!(
                    "Unsupported reset_type '{}'. Core reset is exposed as 'hardware' by this server.",
                    args.reset_type
                ),
                None,
            ));
        }

        let session_arc = self.get_session(&args.session_id).await?;

        let mut backend = session_arc.backend.lock().await;
        backend.reset(args.halt_after_reset).await.map_err(|e| {
            error!(
                "Failed to reset target for session {}: {}",
                args.session_id, e
            );
            McpError::internal_error(format!("Failed to reset target: {}", e), None)
        })?;
        let snap = backend.snapshot().await.ok();
        drop(backend);

        let (pc, sp) = snap.map(|s| (s.pc, s.sp)).unwrap_or((0, 0));
        let message = format!(
            "Target reset completed successfully.\n\n\
            Session ID: {}\n\
            Reset type: {}\n\
            Halted after reset: {}\n\
            PC: 0x{:08X}\n\
            SP: 0x{:08X}\n\
            State: {}\n",
            args.session_id,
            args.reset_type,
            args.halt_after_reset,
            pc,
            sp,
            if args.halt_after_reset {
                "Halted"
            } else {
                "Running"
            }
        );
        info!("Reset completed for session: {}", args.session_id);
        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    #[tool(description = "Execute a single instruction step")]
    async fn step(
        &self,
        Parameters(args): Parameters<StepArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Single stepping target for session: {}", args.session_id);
        let session_arc = self.get_session(&args.session_id).await?;

        let mut backend = session_arc.backend.lock().await;
        backend.step().await.map_err(|e| {
            error!(
                "Failed to step target for session {}: {}",
                args.session_id, e
            );
            McpError::internal_error(format!("Failed to step target: {}", e), None)
        })?;
        let snap = backend.snapshot().await.ok();
        drop(backend);

        let (pc, sp) = snap.map(|s| (s.pc, s.sp)).unwrap_or((0, 0));
        let message = format!(
            "Single step completed successfully!\n\n\
            Session ID: {}\n\
            PC: 0x{:08X}\n\
            SP: 0x{:08X}\n\
            State: Halted\n",
            args.session_id, pc, sp
        );
        info!("Step completed for session: {}", args.session_id);
        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    #[tool(description = "Get current status of the target CPU and debug session")]
    async fn get_status(
        &self,
        Parameters(args): Parameters<GetStatusArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Getting status for session: {}", args.session_id);
        let session_arc = self.get_session(&args.session_id).await?;

        let snap = {
            let mut backend = session_arc.backend.lock().await;
            backend.snapshot().await.map_err(|e| {
                error!(
                    "Failed to get status for session {}: {}",
                    args.session_id, e
                );
                McpError::internal_error(format!("Failed to get core status: {}", e), None)
            })?
        };

        let is_halted = snap.state == CoreState::Halted;
        let message = format!(
            "Debug Session Status\n\n\
            Core Information:\n\
            - PC: 0x{:08X}\n\
            - SP: 0x{:08X}\n\
            - State: {}\n\
            - Halt reason: {}\n\n\
            Session Information:\n\
            - ID: {}\n\
            - Connected: true\n\
            - Target: {}\n\
            - Probe: {}\n\
            - Duration: {:.1} minutes\n",
            snap.pc,
            snap.sp,
            if is_halted { "Halted" } else { "Running" },
            snap.halt_reason.as_deref().unwrap_or("N/A"),
            args.session_id,
            session_arc.target_chip,
            session_arc.probe_identifier,
            (chrono::Utc::now() - session_arc.created_at).num_seconds() as f64 / 60.0
        );
        Ok(CallToolResult::success(vec![Content::text(message)]))
    }
}
