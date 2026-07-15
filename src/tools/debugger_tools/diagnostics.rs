//! AI-facing crash-diagnosis tools.
//!
//! Design for flagship models: aggregate the scattered reads into ONE call and
//! return a compact, structured evidence bundle (raw register values + which
//! bits are set). We do NOT assert a root cause or embed a teaching decision
//! tree — the model reasons over the evidence. This saves context (one call
//! instead of ~6 `read_memory`) without over-teaching.

use rmcp::{handler::server::tool::Parameters, model::*, tool, tool_router, ErrorData as McpError};
use std::future::Future;
use tracing::{debug, info};

use super::session::EmbeddedDebuggerToolHandler;
use crate::backend::{CoreRegId, DebugBackend};
use crate::tools::types::*;

// ARMv7-M System Control Block fault registers (identical on all Cortex-M).
// Source: ARMv7-M Architecture Reference Manual, System Control Block.
const CPUID: u64 = 0xE000_ED00;
const ICSR: u64 = 0xE000_ED04;
const SHCSR: u64 = 0xE000_ED24;
const CFSR: u64 = 0xE000_ED28;
const HFSR: u64 = 0xE000_ED2C;
const MMFAR: u64 = 0xE000_ED34;
const BFAR: u64 = 0xE000_ED38;

// Actionable CFSR bits (bit index -> name). Compact evidence, not a tutorial.
const CFSR_BITS: &[(u32, &str)] = &[
    // MemManage fault (MFSR, byte 0)
    (0, "IACCVIOL"),
    (1, "DACCVIOL"),
    (3, "MUNSTKERR"),
    (4, "MSTKERR"),
    (7, "MMARVALID"),
    // BusFault (BFSR, byte 1)
    (8, "IBUSERR"),
    (9, "PRECISERR"),
    (10, "IMPRECISERR"),
    (11, "UNSTKERR"),
    (12, "STKERR"),
    (15, "BFARVALID"),
    // UsageFault (UFSR, bytes 2-3)
    (16, "UNDEFINSTR"),
    (17, "INVSTATE"),
    (18, "INVPC"),
    (19, "NOCP"),
    (24, "UNALIGNED"),
    (25, "DIVBYZERO"),
];

const HFSR_BITS: &[(u32, &str)] = &[(1, "VECTTBL"), (30, "FORCED"), (31, "DEBUGEVT")];

fn set_flags(value: u32, table: &[(u32, &str)]) -> Vec<String> {
    table
        .iter()
        .filter(|(bit, _)| value & (1 << bit) != 0)
        .map(|(_, name)| name.to_string())
        .collect()
}

async fn read_word_opt(backend: &mut Box<dyn DebugBackend>, addr: u64) -> Option<u32> {
    backend.read_word(addr).await.ok()
}

fn hex_or_null(v: Option<u32>) -> serde_json::Value {
    match v {
        Some(x) => serde_json::Value::String(format!("0x{:08X}", x)),
        None => serde_json::Value::Null,
    }
}

#[tool_router(router = diagnostics_tool_router, vis = "pub")]
impl EmbeddedDebuggerToolHandler {
    #[tool(
        description = "Aggregate crash evidence in one call: read the Cortex-M SCB fault registers \
        (CFSR/HFSR/MMFAR/BFAR/SHCSR/CPUID/ICSR) plus PC/SP/LR and return a compact structured \
        bundle with the set fault bits. Returns raw evidence for you to reason over; it does not \
        assert a root cause. Halt the target first for meaningful values."
    )]
    async fn diagnose_fault(
        &self,
        Parameters(args): Parameters<DiagnoseFaultArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Diagnosing fault for session: {}", args.session_id);
        let session_arc = self.get_session(&args.session_id).await?;

        let mut backend = session_arc.backend.lock().await;
        let backend_kind = backend.kind().to_string();

        let cfsr = read_word_opt(&mut backend, CFSR).await;
        let hfsr = read_word_opt(&mut backend, HFSR).await;
        let mmfar = read_word_opt(&mut backend, MMFAR).await;
        let bfar = read_word_opt(&mut backend, BFAR).await;
        let shcsr = read_word_opt(&mut backend, SHCSR).await;
        let cpuid = read_word_opt(&mut backend, CPUID).await;
        let icsr = read_word_opt(&mut backend, ICSR).await;

        let pc = backend.core_reg(CoreRegId::Pc).await.ok();
        let sp = backend.core_reg(CoreRegId::Sp).await.ok();
        let lr = backend.core_reg(CoreRegId::Lr).await.ok();
        drop(backend);

        let cfsr_flags = cfsr.map(|v| set_flags(v, CFSR_BITS)).unwrap_or_default();
        let hfsr_flags = hfsr.map(|v| set_flags(v, HFSR_BITS)).unwrap_or_default();

        // Fault addresses are only meaningful when the matching *ARVALID bit is set.
        let mmfar_valid = cfsr.map(|v| v & (1 << 7) != 0).unwrap_or(false);
        let bfar_valid = cfsr.map(|v| v & (1 << 15) != 0).unwrap_or(false);
        let mmfar_value = if mmfar_valid {
            hex_or_null(mmfar)
        } else {
            serde_json::Value::Null
        };
        let bfar_value = if bfar_valid {
            hex_or_null(bfar)
        } else {
            serde_json::Value::Null
        };

        let report = serde_json::json!({
            "session": args.session_id,
            "backend": backend_kind,
            "target": session_arc.target_chip,
            "core": {
                "pc": hex_or_null(pc),
                "sp": hex_or_null(sp),
                "lr": hex_or_null(lr),
            },
            "scb_raw": {
                "cfsr": hex_or_null(cfsr),
                "hfsr": hex_or_null(hfsr),
                "shcsr": hex_or_null(shcsr),
                "cpuid": hex_or_null(cpuid),
                "icsr": hex_or_null(icsr),
                "mmfar": hex_or_null(mmfar),
                "bfar": hex_or_null(bfar),
            },
            "cfsr_flags": cfsr_flags,
            "hfsr_flags": hfsr_flags,
            "fault_address": {
                "mmfar": mmfar_value,
                "bfar": bfar_value,
            },
            "note": "Raw Cortex-M fault evidence. No root cause asserted; reason from the set bits and fault address. Values are meaningful only when the core is halted in the fault context."
        });

        let text = serde_json::to_string_pretty(&report)
            .unwrap_or_else(|e| format!("{{\"error\":\"serialize failed: {}\"}}", e));

        info!("Diagnose completed for session: {}", args.session_id);
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_divbyzero_and_valid_bits() {
        // DIVBYZERO (bit 25) + MMARVALID (bit 7)
        let cfsr = (1 << 25) | (1 << 7);
        let flags = set_flags(cfsr, CFSR_BITS);
        assert!(flags.contains(&"DIVBYZERO".to_string()));
        assert!(flags.contains(&"MMARVALID".to_string()));
        assert!(!flags.contains(&"UNALIGNED".to_string()));
    }

    #[test]
    fn decodes_hfsr_forced() {
        let flags = set_flags(1 << 30, HFSR_BITS);
        assert_eq!(flags, vec!["FORCED".to_string()]);
    }
}
