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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CoreRegId, CoreState, DebugBackend};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn checksum(bytes: &[u8]) -> u8 {
        bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b))
    }

    /// Read one RSP packet payload from a socket ($payload#cc). None on EOF.
    async fn read_packet(sock: &mut TcpStream) -> Option<String> {
        let mut b = [0u8; 1];
        loop {
            if sock.read_exact(&mut b).await.is_err() {
                return None;
            }
            if b[0] == b'$' {
                break;
            }
        }
        let mut data = Vec::new();
        loop {
            sock.read_exact(&mut b).await.ok()?;
            if b[0] == b'#' {
                break;
            }
            data.push(b[0]);
        }
        let mut cks = [0u8; 2];
        sock.read_exact(&mut cks).await.ok()?;
        String::from_utf8(data).ok()
    }

    async fn send_packet(sock: &mut TcpStream, payload: &str) {
        let framed = format!("${}#{:02x}", payload, checksum(payload.as_bytes()));
        sock.write_all(framed.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();
    }

    /// Minimal GDB-RSP server that answers the subset our client uses.
    async fn mock_rsp_server(listener: TcpListener) {
        let (mut sock, _) = listener.accept().await.unwrap();
        loop {
            let pkt = match read_packet(&mut sock).await {
                Some(p) => p,
                None => break,
            };
            // Ack the client's packet.
            sock.write_all(b"+").await.unwrap();

            // "OK" acknowledges memory writes, monitor (qRcmd) commands, and
            // breakpoint set/clear; the others return specific payloads.
            let resp: &str = if pkt.starts_with('m') {
                "deadbeef" // 4 bytes for a read
            } else if pkt == "pf" {
                "00010008" // PC = 0x08000100, little-endian
            } else if pkt.starts_with('p') {
                "00000000"
            } else if pkt == "?" {
                "T05" // halted
            } else if pkt.starts_with('M')
                || pkt.starts_with("qRcmd")
                || pkt.starts_with('Z')
                || pkt.starts_with('z')
            {
                "OK"
            } else {
                ""
            };

            send_packet(&mut sock, resp).await;

            // Read the client's ack for our response.
            let mut ack = [0u8; 1];
            if sock.read_exact(&mut ack).await.is_err() {
                break;
            }
        }
    }

    #[tokio::test]
    async fn openocd_backend_drives_rsp_end_to_end() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(mock_rsp_server(listener));

        let mut backend = OpenOcdBackend::connect(&addr).await.expect("connect");
        assert_eq!(backend.kind(), BackendKind::OpenOcd);

        // Memory read: "m..,4" -> "deadbeef".
        let data = backend.read_bytes(0x2000_0000, 4).await.expect("read");
        assert_eq!(data, vec![0xde, 0xad, 0xbe, 0xef]);

        // Memory write: "M..:..." -> OK.
        backend
            .write_bytes(0x2000_0000, &[0x01, 0x02])
            .await
            .expect("write");

        // Register read: "pf" (r15/PC) -> little-endian 0x08000100.
        let pc = backend.core_reg(CoreRegId::Pc).await.expect("reg");
        assert_eq!(pc, 0x0800_0100);

        // Status: "?" -> T05 => Halted.
        assert_eq!(backend.status().await.unwrap(), CoreState::Halted);

        // Control via monitor (qRcmd) -> OK.
        backend.halt().await.expect("halt");
        backend.run().await.expect("run");

        // Breakpoints: Z1/z1 -> OK.
        backend
            .set_hw_breakpoint(0x0800_0100)
            .await
            .expect("set bp");
        backend
            .clear_hw_breakpoint(0x0800_0100)
            .await
            .expect("clear bp");

        drop(backend);
        let _ = server.await;
    }
}
