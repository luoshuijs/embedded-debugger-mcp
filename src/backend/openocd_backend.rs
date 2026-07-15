//! OpenOCD engine via GDB Remote Serial Protocol (optional backend).
//!
//! Requires an already-running `openocd` exposing its GDB port (default :3333),
//! e.g. `openocd -f interface/stlink.cfg -f target/stm32f4x.cfg`. Memory access
//! uses native RSP (`m`/`M`); execution control uses OpenOCD `monitor` commands
//! (synchronous, so `run` returns promptly instead of blocking like `c`).

use async_trait::async_trait;

use super::rsp::{decode_hex, encode_hex, RspClient};
use super::{BackendKind, CoreRegId, CoreState, DebugBackend};
use crate::error::{DebugError, Result};

pub struct OpenOcdBackend {
    rsp: RspClient,
    address: String,
}

impl OpenOcdBackend {
    pub async fn connect(address: &str) -> Result<Self> {
        let rsp = RspClient::connect(address).await?;
        Ok(Self {
            rsp,
            address: address.to_string(),
        })
    }

    pub fn address(&self) -> &str {
        &self.address
    }
}

/// gdb register numbers for the ARM core profile (stable subset).
fn reg_number(reg: CoreRegId) -> u32 {
    match reg {
        CoreRegId::Sp => 13,
        CoreRegId::Lr => 14,
        CoreRegId::Pc => 15,
    }
}

fn expect_ok(reply: &str, what: &str) -> Result<()> {
    if reply == "OK" {
        Ok(())
    } else if let Some(code) = reply.strip_prefix('E') {
        Err(DebugError::InternalError(format!(
            "OpenOCD {} error E{}",
            what, code
        )))
    } else {
        // Some stubs answer empty for unsupported; treat non-error as success.
        Ok(())
    }
}

#[async_trait]
impl DebugBackend for OpenOcdBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::OpenOcd
    }

    async fn read_bytes(&mut self, address: u64, len: usize) -> Result<Vec<u8>> {
        let reply = self
            .rsp
            .command(&format!("m{:x},{:x}", address, len))
            .await?;
        if let Some(code) = reply.strip_prefix('E') {
            return Err(DebugError::MemoryAccessFailed(format!(
                "OpenOCD read 0x{:08X} error E{}",
                address, code
            )));
        }
        decode_hex(&reply)
    }

    async fn write_bytes(&mut self, address: u64, data: &[u8]) -> Result<()> {
        let reply = self
            .rsp
            .command(&format!(
                "M{:x},{:x}:{}",
                address,
                data.len(),
                encode_hex(data)
            ))
            .await?;
        expect_ok(&reply, "write")
    }

    async fn halt(&mut self) -> Result<()> {
        self.rsp.monitor("halt").await.map(|_| ())
    }

    async fn run(&mut self) -> Result<()> {
        self.rsp.monitor("resume").await.map(|_| ())
    }

    async fn step(&mut self) -> Result<()> {
        self.rsp.monitor("step").await.map(|_| ())
    }

    async fn reset(&mut self, halt_after: bool) -> Result<()> {
        let cmd = if halt_after {
            "reset halt"
        } else {
            "reset run"
        };
        self.rsp.monitor(cmd).await.map(|_| ())
    }

    async fn core_reg(&mut self, reg: CoreRegId) -> Result<u32> {
        let reply = self.rsp.command(&format!("p{:x}", reg_number(reg))).await?;
        if let Some(code) = reply.strip_prefix('E') {
            return Err(DebugError::InternalError(format!(
                "OpenOCD read reg error E{}",
                code
            )));
        }
        let bytes = decode_hex(&reply)?;
        if bytes.len() < 4 {
            return Err(DebugError::InternalError(format!(
                "OpenOCD register reply too short: {:?}",
                reply
            )));
        }
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    async fn status(&mut self) -> Result<CoreState> {
        let reply = self.rsp.command("?").await?;
        Ok(if reply.starts_with('T') || reply.starts_with('S') {
            CoreState::Halted
        } else {
            CoreState::Unknown
        })
    }

    async fn set_hw_breakpoint(&mut self, address: u64) -> Result<()> {
        let reply = self.rsp.command(&format!("Z1,{:x},2", address)).await?;
        expect_ok(&reply, "set breakpoint")
    }

    async fn clear_hw_breakpoint(&mut self, address: u64) -> Result<()> {
        let reply = self.rsp.command(&format!("z1,{:x},2", address)).await?;
        expect_ok(&reply, "clear breakpoint")
    }
}
