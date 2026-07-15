//! Minimal GDB Remote Serial Protocol (RSP) client over an async TCP socket.
//!
//! Only the subset needed to drive an OpenOCD (or any gdbserver) target is
//! implemented: packet framing + checksum, memory read/write, register read,
//! `monitor` (qRcmd) passthrough, breakpoints, and target interrupt.
//!
//! Using RSP (rather than shelling out `openocd -c "TCL"`) means the model
//! makes one structured tool call instead of typing commands and parsing text.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::{DebugError, Result};

/// RSP checksum: modulo-256 sum of the payload bytes.
pub(crate) fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |acc, b| acc.wrapping_add(*b))
}

/// Encode raw bytes as lowercase hex.
pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Decode a lowercase/uppercase hex string into bytes.
pub(crate) fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err(DebugError::InternalError(format!(
            "RSP hex payload has odd length: {:?}",
            s
        )));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| DebugError::InternalError(format!("RSP invalid hex {:?}: {}", s, e)))
        })
        .collect()
}

pub struct RspClient {
    stream: TcpStream,
}

impl RspClient {
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await.map_err(|e| {
            DebugError::ConnectionFailed(format!("OpenOCD RSP connect to {}: {}", addr, e))
        })?;
        Ok(Self { stream })
    }

    /// Send a framed packet `$payload#cc` and wait for the `+` acknowledgement.
    pub async fn send_packet(&mut self, payload: &str) -> Result<()> {
        let framed = format!("${}#{:02x}", payload, checksum(payload.as_bytes()));
        self.stream.write_all(framed.as_bytes()).await?;
        self.stream.flush().await?;
        self.read_ack().await
    }

    async fn read_ack(&mut self) -> Result<()> {
        let mut b = [0u8; 1];
        loop {
            self.stream.read_exact(&mut b).await?;
            match b[0] {
                b'+' => return Ok(()),
                b'-' => {
                    return Err(DebugError::InternalError(
                        "RSP peer sent NACK ('-')".to_string(),
                    ))
                }
                _ => continue,
            }
        }
    }

    /// Receive one framed packet, returning its unescaped payload. Sends `+`.
    pub async fn recv_packet(&mut self) -> Result<String> {
        let mut byte = [0u8; 1];
        // Skip until start-of-packet.
        loop {
            self.stream.read_exact(&mut byte).await?;
            if byte[0] == b'$' {
                break;
            }
        }
        let mut data: Vec<u8> = Vec::new();
        loop {
            self.stream.read_exact(&mut byte).await?;
            if byte[0] == b'#' {
                break;
            }
            if byte[0] == b'}' {
                // RSP escape: next byte XOR 0x20.
                self.stream.read_exact(&mut byte).await?;
                data.push(byte[0] ^ 0x20);
            } else {
                data.push(byte[0]);
            }
        }
        let mut cks = [0u8; 2];
        self.stream.read_exact(&mut cks).await?;
        self.stream.write_all(b"+").await?;
        self.stream.flush().await?;
        String::from_utf8(data)
            .map_err(|e| DebugError::InternalError(format!("RSP non-utf8 payload: {}", e)))
    }

    /// Send a packet and return the single reply payload.
    pub async fn command(&mut self, payload: &str) -> Result<String> {
        self.send_packet(payload).await?;
        self.recv_packet().await
    }

    /// Interrupt (halt) a running target: raw 0x03, then drain the stop reply.
    pub async fn interrupt(&mut self) -> Result<()> {
        self.stream.write_all(&[0x03]).await?;
        self.stream.flush().await?;
        let _ = self.recv_packet().await?;
        Ok(())
    }

    /// Run an OpenOCD monitor command via `qRcmd`, collecting any `O`-output.
    pub async fn monitor(&mut self, cmd: &str) -> Result<String> {
        self.send_packet(&format!("qRcmd,{}", encode_hex(cmd.as_bytes())))
            .await?;
        let mut out = String::new();
        loop {
            let pkt = self.recv_packet().await?;
            if pkt == "OK" || pkt.is_empty() {
                break;
            }
            if let Some(rest) = pkt.strip_prefix('O') {
                if let Ok(bytes) = decode_hex(rest) {
                    out.push_str(&String::from_utf8_lossy(&bytes));
                }
                continue;
            }
            if pkt.starts_with('E') {
                return Err(DebugError::InternalError(format!(
                    "OpenOCD monitor '{}' error: {}",
                    cmd, pkt
                )));
            }
            out.push_str(&pkt);
            break;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_gdb_spec() {
        // '$m0,4#' example: checksum of "m0,4"
        assert_eq!(
            checksum(b"m0,4"),
            b'm'.wrapping_add(b'0')
                .wrapping_add(b',')
                .wrapping_add(b'4')
        );
    }

    #[test]
    fn hex_roundtrip() {
        let bytes = [0x00u8, 0x01, 0x08, 0xff, 0x7f];
        assert_eq!(decode_hex(&encode_hex(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn decode_hex_rejects_odd_length() {
        assert!(decode_hex("abc").is_err());
    }

    #[test]
    fn decode_little_endian_register() {
        // 'p' reply for PC=0x08000100 is target-endian (LE) => "00010008"
        let raw = decode_hex("00010008").unwrap();
        let pc = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        assert_eq!(pc, 0x0800_0100);
    }
}
