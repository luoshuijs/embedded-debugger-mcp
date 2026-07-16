//! probe-rs native engine (default backend).

use std::sync::Arc;

use async_trait::async_trait;
use probe_rs::{CoreStatus, MemoryInterface, RegisterValue, Session};
use tokio::sync::Mutex;

use super::{BackendKind, CoreRegId, CoreState, DebugBackend};
use crate::error::{DebugError, Result};

/// Wraps a shared probe-rs `Session`. The same `Arc` is also held by the
/// session record so probe-rs-specific tools (flash, RTT) can keep using it.
pub struct ProbeRsBackend {
    session: Arc<Mutex<Session>>,
}

impl ProbeRsBackend {
    pub fn new(session: Arc<Mutex<Session>>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl DebugBackend for ProbeRsBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::ProbeRs
    }

    async fn read_bytes(&mut self, address: u64, len: usize) -> Result<Vec<u8>> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        let mut buf = vec![0u8; len];
        core.read(address, &mut buf)
            .map_err(|e| DebugError::MemoryAccessFailed(e.to_string()))?;
        Ok(buf)
    }

    async fn write_bytes(&mut self, address: u64, data: &[u8]) -> Result<()> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        core.write(address, data)
            .map_err(|e| DebugError::MemoryAccessFailed(e.to_string()))?;
        Ok(())
    }

    async fn halt(&mut self) -> Result<()> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        core.halt(std::time::Duration::from_millis(1000))
            .map_err(|e| DebugError::InternalError(format!("halt failed: {}", e)))?;
        Ok(())
    }

    async fn run(&mut self) -> Result<()> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        core.run()
            .map_err(|e| DebugError::InternalError(format!("run failed: {}", e)))?;
        Ok(())
    }

    async fn step(&mut self) -> Result<()> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        core.step()
            .map_err(|e| DebugError::InternalError(format!("step failed: {}", e)))?;
        Ok(())
    }

    async fn reset(&mut self, halt_after: bool) -> Result<()> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        if halt_after {
            core.reset_and_halt(std::time::Duration::from_millis(1000))
                .map_err(|e| DebugError::InternalError(format!("reset_and_halt failed: {}", e)))?;
        } else {
            core.reset()
                .map_err(|e| DebugError::InternalError(format!("reset failed: {}", e)))?;
        }
        Ok(())
    }

    async fn core_reg(&mut self, reg: CoreRegId) -> Result<u32> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        let id = match reg {
            CoreRegId::Pc => core.program_counter(),
            CoreRegId::Sp => core.stack_pointer(),
            CoreRegId::Lr => core.return_address(),
        };
        let value: RegisterValue = core
            .read_core_reg(id)
            .map_err(|e| DebugError::InternalError(format!("read core reg failed: {}", e)))?;
        Ok(value.try_into().unwrap_or(0u32))
    }

    async fn status(&mut self) -> Result<CoreState> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        let status = core
            .status()
            .map_err(|e| DebugError::InternalError(format!("status failed: {}", e)))?;
        Ok(match status {
            CoreStatus::Halted(_) => CoreState::Halted,
            CoreStatus::Running => CoreState::Running,
            _ => CoreState::Unknown,
        })
    }

    async fn set_hw_breakpoint(&mut self, address: u64) -> Result<()> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        core.set_hw_breakpoint(address)
            .map_err(|e| DebugError::InternalError(format!("set breakpoint failed: {}", e)))?;
        Ok(())
    }

    async fn clear_hw_breakpoint(&mut self, address: u64) -> Result<()> {
        let mut session = self.session.lock().await;
        let mut core = session.core(0)?;
        core.clear_hw_breakpoint(address)
            .map_err(|e| DebugError::InternalError(format!("clear breakpoint failed: {}", e)))?;
        Ok(())
    }
}
