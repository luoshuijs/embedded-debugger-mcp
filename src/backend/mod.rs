//! Debug backend abstraction (dual-engine).
//!
//! A single `DebugBackend` trait unifies the two execution engines so the MCP
//! tool layer speaks one API and never learns two. The concrete engine (probe-rs
//! native, or OpenOCD via GDB Remote Serial Protocol) is an implementation
//! detail chosen at `connect` time.

use async_trait::async_trait;

use crate::error::Result;

mod openocd_backend;
mod probe_rs_backend;
pub mod rsp;

pub use openocd_backend::OpenOcdBackend;
pub use probe_rs_backend::ProbeRsBackend;

/// Which engine backs a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    ProbeRs,
    OpenOcd,
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendKind::ProbeRs => write!(f, "probe-rs"),
            BackendKind::OpenOcd => write!(f, "openocd"),
        }
    }
}

/// Core registers a backend can resolve by role (architecture-neutral subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreRegId {
    Pc,
    Sp,
    Lr,
}

/// Coarse core execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreState {
    Halted,
    Running,
    Unknown,
}

impl std::fmt::Display for CoreState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreState::Halted => write!(f, "Halted"),
            CoreState::Running => write!(f, "Running"),
            CoreState::Unknown => write!(f, "Unknown"),
        }
    }
}

/// A snapshot of the most commonly needed core state, gathered in one shot to
/// save the model a round of individual reads.
#[derive(Debug, Clone)]
pub struct CoreSnapshot {
    pub pc: u32,
    pub sp: u32,
    pub state: CoreState,
    pub halt_reason: Option<String>,
}

/// The unified debug engine interface. probe-rs and OpenOCD both implement it.
#[async_trait]
pub trait DebugBackend: Send {
    fn kind(&self) -> BackendKind;

    async fn read_bytes(&mut self, address: u64, len: usize) -> Result<Vec<u8>>;
    async fn write_bytes(&mut self, address: u64, data: &[u8]) -> Result<()>;

    async fn halt(&mut self) -> Result<()>;
    async fn run(&mut self) -> Result<()>;
    async fn step(&mut self) -> Result<()>;
    async fn reset(&mut self, halt_after: bool) -> Result<()>;

    async fn core_reg(&mut self, reg: CoreRegId) -> Result<u32>;
    async fn status(&mut self) -> Result<CoreState>;

    async fn set_hw_breakpoint(&mut self, address: u64) -> Result<()>;
    async fn clear_hw_breakpoint(&mut self, address: u64) -> Result<()>;

    /// Read a single little-endian 32-bit word. Default via `read_bytes`.
    async fn read_word(&mut self, address: u64) -> Result<u32> {
        let b = self.read_bytes(address, 4).await?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// One-shot core snapshot (state + PC + SP), tolerant of missing registers.
    async fn snapshot(&mut self) -> Result<CoreSnapshot> {
        let state = self.status().await.unwrap_or(CoreState::Unknown);
        let pc = self.core_reg(CoreRegId::Pc).await.unwrap_or(0);
        let sp = self.core_reg(CoreRegId::Sp).await.unwrap_or(0);
        Ok(CoreSnapshot {
            pc,
            sp,
            state,
            halt_reason: None,
        })
    }
}
