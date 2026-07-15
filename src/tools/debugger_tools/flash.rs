use rmcp::{handler::server::tool::Parameters, model::*, tool, tool_router, ErrorData as McpError};
use std::future::Future;
use tracing::{debug, error, info, warn};

use super::formatting::{parse_address, parse_data};
use super::session::EmbeddedDebuggerToolHandler;
use crate::tools::types::*;

#[tool_router(router = flash_tool_router, vis = "pub")]
impl EmbeddedDebuggerToolHandler {
    // =============================================================================
    // Flash Programming Tools (4 tools)
    // =============================================================================

    #[tool(description = "Erase flash memory sectors or entire chip")]
    async fn flash_erase(
        &self,
        Parameters(args): Parameters<FlashEraseArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Flash erase for session: {}, type: {}",
            args.session_id, args.erase_type
        );

        self.ensure_flash_erase_allowed()?;
        let session_arc = self.get_session(&args.session_id).await?;

        // Parse erase type and parameters
        let erase_type = match args.erase_type.as_str() {
            "all" => crate::flash::EraseType::All,
            "sectors" => {
                let address = match args.address {
                    Some(addr_str) => {
                        parse_address(&addr_str).map_err(|e| McpError::internal_error(e, None))?
                    }
                    None => {
                        return Err(McpError::internal_error(
                            "Address required for sector erase".to_string(),
                            None,
                        ))
                    }
                };
                let size = match args.size {
                    Some(sz) => sz as usize,
                    None => {
                        return Err(McpError::internal_error(
                            "Size required for sector erase".to_string(),
                            None,
                        ))
                    }
                };
                crate::flash::EraseType::Sectors { address, size }
            }
            _ => {
                return Err(McpError::internal_error(
                    format!("Invalid erase type: {}", args.erase_type),
                    None,
                ))
            }
        };

        // Perform erase operation
        {
            let probe_session = session_arc.probe_session()?;
            let mut session = probe_session.lock().await;
            match crate::flash::FlashManager::erase_flash(&mut session, erase_type).await {
                Ok(result) => {
                    let message = format!(
                        "Flash erase completed successfully.\n\n\
                        Session ID: {}\n\
                        Erase Type: {}\n\
                        Duration: {}ms\n\
                        {}\n\n\
                        Flash memory has been erased and is ready for programming.",
                        args.session_id,
                        args.erase_type,
                        result.erase_time_ms,
                        match result.sectors_erased {
                            Some(count) => format!("Sectors Erased: {}", count),
                            None => "Full chip erased".to_string(),
                        }
                    );

                    info!("Flash erase completed for session: {}", args.session_id);
                    Ok(CallToolResult::success(vec![Content::text(message)]))
                }
                Err(e) => {
                    error!("Flash erase failed for session {}: {}", args.session_id, e);
                    let error_msg = format!(
                        "Flash erase failed\n\n\
                        Session ID: {}\n\
                        Error: {}\n\n\
                        Suggestions:\n\
                        - Check if flash is write-protected\n\
                        - Ensure target is halted\n\
                        - Verify flash address range",
                        args.session_id, e
                    );
                    Err(McpError::internal_error(error_msg, None))
                }
            }
        }
    }

    #[tool(description = "Program file to flash memory (supports ELF, HEX, BIN)")]
    async fn flash_program(
        &self,
        Parameters(args): Parameters<FlashProgramArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Flash program for session: {}, file: {}",
            args.session_id, args.file_path
        );

        self.ensure_flash_erase_allowed()?;

        let session_arc = self.get_session(&args.session_id).await?;

        // Parse file path and format
        let file_path =
            self.resolve_allowed_file_path(&args.file_path, self.config.flash.max_binary_size)?;
        let format = match args.format.as_str() {
            "auto" => crate::flash::FileFormat::Auto,
            "elf" => crate::flash::FileFormat::Elf,
            "hex" => crate::flash::FileFormat::Hex,
            "bin" => crate::flash::FileFormat::Bin,
            _ => {
                return Err(McpError::internal_error(
                    format!("Unsupported format: {}", args.format),
                    None,
                ))
            }
        };

        // Parse base address if provided
        let base_address = if let Some(addr_str) = args.base_address {
            Some(parse_address(&addr_str).map_err(|e| McpError::internal_error(e, None))?)
        } else {
            None
        };

        // Perform programming operation
        {
            let probe_session = session_arc.probe_session()?;
            let mut session = probe_session.lock().await;
            let verify = args.verify || self.config.flash.verify_after_program;
            match crate::flash::FlashManager::program_file(
                &mut session,
                &file_path,
                format,
                base_address,
                verify,
            )
            .await
            {
                Ok(result) => {
                    let message = format!(
                        "Flash programming completed successfully.\n\n\
                        Session ID: {}\n\
                        File: {}\n\
                        Format: {}\n\
                        Bytes Programmed: {}\n\
                        Duration: {}ms\n\
                        Verification: {}\n\n\
                        Firmware has been programmed to flash memory.",
                        args.session_id,
                        file_path.display(),
                        args.format,
                        result.bytes_programmed,
                        result.programming_time_ms,
                        match result.verification_result {
                            Some(true) => "Passed",
                            Some(false) => "Failed",
                            None => "Not performed",
                        }
                    );

                    info!(
                        "Flash programming completed for session: {}",
                        args.session_id
                    );
                    Ok(CallToolResult::success(vec![Content::text(message)]))
                }
                Err(e) => {
                    error!(
                        "Flash programming failed for session {}: {}",
                        args.session_id, e
                    );
                    let error_msg = format!(
                        "Flash programming failed\n\n\
                        Session ID: {}\n\
                        File: {}\n\
                        Error: {}\n\n\
                        Suggestions:\n\
                        - Check file exists and is readable\n\
                        - Verify file format is correct\n\
                        - Ensure flash is erased first\n\
                        - Check target memory map",
                        args.session_id, args.file_path, e
                    );
                    Err(McpError::internal_error(error_msg, None))
                }
            }
        }
    }

    #[tool(description = "Verify flash memory contents")]
    async fn flash_verify(
        &self,
        Parameters(args): Parameters<FlashVerifyArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Flash verify for session: {}", args.session_id);

        let session_arc = {
            let sessions = self.sessions.read().await;
            match sessions.get(&args.session_id) {
                Some(session) => session.clone(),
                None => {
                    let error_msg = format!("Session '{}' not found\n\nUse 'connect' to establish a debug session first", args.session_id);
                    return Err(McpError::internal_error(error_msg, None));
                }
            }
        };

        // Parse address
        let address =
            parse_address(&args.address).map_err(|e| McpError::internal_error(e, None))?;

        // Get expected data
        let expected_data = if let Some(file_path) = &args.file_path {
            let file_path =
                self.resolve_allowed_file_path(file_path, self.config.flash.max_binary_size)?;
            let extension = file_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase());
            if matches!(extension.as_deref(), Some("elf" | "hex")) {
                return Err(McpError::internal_error(
                    format!(
                        "flash_verify compares raw bytes only; '{}' must be verified with a raw BIN file or hex data.",
                        file_path.display()
                    ),
                    None,
                ));
            }
            std::fs::read(&file_path).map_err(|e| {
                McpError::internal_error(
                    format!("Failed to read file {}: {}", file_path.display(), e),
                    None,
                )
            })?
        } else if let Some(hex_data) = &args.data {
            // Parse hex data
            match parse_data(hex_data, "hex") {
                Ok(data) => data,
                Err(e) => {
                    return Err(McpError::internal_error(
                        format!("Invalid hex data: {}", e),
                        None,
                    ))
                }
            }
        } else {
            return Err(McpError::internal_error(
                "Either file_path or data must be provided".to_string(),
                None,
            ));
        };

        let verify_size = args.size as usize;
        if expected_data.len() < verify_size {
            return Err(McpError::internal_error(
                format!(
                    "Expected data has {} bytes, fewer than requested verify size {}.",
                    expected_data.len(),
                    verify_size
                ),
                None,
            ));
        }

        let expected_data = if expected_data.len() > verify_size {
            &expected_data[..verify_size]
        } else {
            &expected_data
        };

        self.ensure_memory_read_allowed_for_target(
            &session_arc.target_chip,
            address,
            expected_data.len(),
        )?;

        // Perform verification
        {
            let probe_session = session_arc.probe_session()?;
            let mut session = probe_session.lock().await;
            match crate::flash::FlashManager::verify_flash(&mut session, expected_data, address)
                .await
            {
                Ok(result) => {
                    let message = if result.success {
                        format!(
                            "Flash verification successful.\n\n\
                            Session ID: {}\n\
                            Address: 0x{:08X}\n\
                            Bytes Verified: {}\n\n\
                            All flash contents match expected data.",
                            args.session_id, address, result.bytes_verified
                        )
                    } else {
                        let mut message = format!(
                            "Flash verification failed.\n\n\
                            Session ID: {}\n\
                            Address: 0x{:08X}\n\
                            Bytes Verified: {}\n\
                            Mismatches: {}\n\n\
                            First {} mismatches:\n",
                            args.session_id,
                            address,
                            result.bytes_verified,
                            result.mismatches.len(),
                            std::cmp::min(10, result.mismatches.len())
                        );

                        for (i, mismatch) in result.mismatches.iter().take(10).enumerate() {
                            message.push_str(&format!(
                                "  {}. 0x{:08X}: expected 0x{:02X}, got 0x{:02X}\n",
                                i + 1,
                                mismatch.address,
                                mismatch.expected,
                                mismatch.actual
                            ));
                        }

                        if result.mismatches.len() > 10 {
                            message.push_str(&format!(
                                "  ... and {} more mismatches\n",
                                result.mismatches.len() - 10
                            ));
                        }

                        message
                    };

                    info!(
                        "Flash verification completed for session: {}",
                        args.session_id
                    );
                    if result.success {
                        Ok(CallToolResult::success(vec![Content::text(message)]))
                    } else {
                        Ok(CallToolResult::error(vec![Content::text(message)]))
                    }
                }
                Err(e) => {
                    error!(
                        "Flash verification failed for session {}: {}",
                        args.session_id, e
                    );
                    let error_msg = format!(
                        "Flash verification error\n\n\
                        Session ID: {}\n\
                        Error: {}",
                        args.session_id, e
                    );
                    Err(McpError::internal_error(error_msg, None))
                }
            }
        }
    }

    #[tool(description = "Firmware deployment helper: erase, program, verify, run, and attach RTT")]
    async fn run_firmware(
        &self,
        Parameters(args): Parameters<RunFirmwareArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Run firmware for session: {}, file: {}",
            args.session_id, args.file_path
        );

        self.ensure_flash_erase_allowed()?;
        let session_arc = self.get_session(&args.session_id).await?;
        let file_path =
            self.resolve_allowed_file_path(&args.file_path, self.config.flash.max_binary_size)?;

        let mut status_messages = Vec::new();
        let start_time = std::time::Instant::now();

        // Step 1: Erase flash
        status_messages.push("Step 1/5: Erasing flash memory...".to_string());
        {
            let probe_session = session_arc.probe_session()?;
            let mut session = probe_session.lock().await;
            match crate::flash::FlashManager::erase_flash(
                &mut session,
                crate::flash::EraseType::All,
            )
            .await
            {
                Ok(_) => status_messages.push("Flash erased successfully".to_string()),
                Err(e) => {
                    let error_msg = format!("Flash erase failed: {}", e);
                    status_messages.push(error_msg.clone());
                    return Err(McpError::internal_error(
                        format!("{}\n\n{}", status_messages.join("\n"), error_msg),
                        None,
                    ));
                }
            }
        }

        // Step 2: Program firmware
        status_messages.push("Step 2/5: Programming firmware...".to_string());
        let format = match args.format.as_str() {
            "auto" => crate::flash::FileFormat::Auto,
            "elf" => crate::flash::FileFormat::Elf,
            "hex" => crate::flash::FileFormat::Hex,
            "bin" => crate::flash::FileFormat::Bin,
            _ => {
                return Err(McpError::internal_error(
                    format!("Unsupported format: {}", args.format),
                    None,
                ))
            }
        };

        {
            let probe_session = session_arc.probe_session()?;
            let mut session = probe_session.lock().await;
            match crate::flash::FlashManager::program_file(
                &mut session,
                &file_path,
                format,
                None,
                self.config.flash.verify_after_program,
            )
            .await
            {
                Ok(result) => {
                    status_messages.push(format!("Programmed {} bytes", result.bytes_programmed))
                }
                Err(e) => {
                    let error_msg = format!("Programming failed: {}", e);
                    status_messages.push(error_msg.clone());
                    return Err(McpError::internal_error(
                        format!("{}\n\n{}", status_messages.join("\n"), error_msg),
                        None,
                    ));
                }
            }
        }

        // Step 3: Reset and run
        if args.reset_after_flash {
            status_messages.push("Step 3/5: Resetting target...".to_string());
            {
                let probe_session = session_arc.probe_session()?;
                let mut session = probe_session.lock().await;
                let mut core = match session.core(0) {
                    Ok(core) => core,
                    Err(e) => {
                        return Err(McpError::internal_error(
                            format!("Failed to get core: {}", e),
                            None,
                        ))
                    }
                };

                match core.reset() {
                    Ok(_) => {
                        status_messages.push("Target reset successfully".to_string());
                        // Run the target
                        match core.run() {
                            Ok(_) => status_messages.push("Target running".to_string()),
                            Err(e) => {
                                let error_msg = format!("Run after reset failed: {}", e);
                                status_messages.push(error_msg.clone());
                                return Err(McpError::internal_error(
                                    format!("{}\n\n{}", status_messages.join("\n"), error_msg),
                                    None,
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        let error_msg = format!("Reset failed: {}", e);
                        status_messages.push(error_msg.clone());
                        return Err(McpError::internal_error(
                            format!("{}\n\n{}", status_messages.join("\n"), error_msg),
                            None,
                        ));
                    }
                }
            }
        }

        // Step 4: Attach RTT (if requested) - Mimic probe-rs run behavior
        if args.attach_rtt {
            status_messages.push("Step 4/5: Attaching RTT...".to_string());

            let timeout = tokio::time::Duration::from_millis(args.rtt_timeout_ms as u64);
            let started = tokio::time::Instant::now();
            let mut rtt_attached = false;
            let mut last_rtt_error = None;
            let mut attempt = 1;

            loop {
                if attempt > 1 {
                    let remaining = timeout.saturating_sub(started.elapsed());
                    if remaining.is_zero() {
                        break;
                    }
                    tokio::time::sleep(remaining.min(tokio::time::Duration::from_millis(500)))
                        .await;
                }

                // Try RTT attachment with different strategies (probe-rs style optimization)
                let mut rtt_manager = session_arc.rtt_manager.lock().await;
                let rtt_result = match attempt {
                    1..=2 => {
                        // First 2 attempts: ELF symbol detection (probe-rs priority method)
                        debug!(
                            "RTT attempt {}: Using ELF symbol detection (probe-rs style)",
                            attempt
                        );
                        rtt_manager
                            .attach_with_elf(session_arc.probe_session()?, &file_path)
                            .await
                    }
                    3..=5 => {
                        // Attempts 3-5: standard attach, let probe-rs auto-scan memory
                        debug!("RTT attempt {}: Using standard memory map scan", attempt);
                        rtt_manager
                            .attach(session_arc.probe_session()?, None, None)
                            .await
                    }
                    6..=7 => {
                        // Attempts 6-7: try STM32G4 specific memory ranges
                        debug!(
                            "RTT attempt {}: Using STM32G4 specific memory ranges",
                            attempt
                        );
                        let stm32g4_ranges = vec![
                            (0x20000000, 0x20004000), // SRAM1 first half: 16KB - most likely RTT location
                            (0x20004000, 0x20008000), // SRAM1 second half: 16KB
                            (0x20008000, 0x2000A000), // SRAM2: 8KB
                        ];
                        rtt_manager
                            .attach(session_arc.probe_session()?, None, Some(stm32g4_ranges))
                            .await
                    }
                    _ => {
                        // Last attempt: try common RTT control block addresses
                        let cb_addr = 0x20000000;
                        debug!(
                            "RTT attempt {}: Using specific control block address 0x{:08X}",
                            attempt, cb_addr
                        );
                        rtt_manager
                            .attach(session_arc.probe_session()?, Some(cb_addr), None)
                            .await
                    }
                };

                match rtt_result {
                    Ok(_) => {
                        let up_channels = rtt_manager.up_channel_count();
                        let down_channels = rtt_manager.down_channel_count();
                        status_messages.push(format!(
                            "RTT attached on attempt {} ({} up, {} down channels)",
                            attempt, up_channels, down_channels
                        ));
                        info!("RTT successfully attached after {} attempts!", attempt);
                        rtt_attached = true;
                        break;
                    }
                    Err(e) => {
                        debug!("RTT attach attempt {} failed: {}", attempt, e);
                        last_rtt_error = Some(e.to_string());
                    }
                }

                if args.rtt_timeout_ms == 0 || started.elapsed() >= timeout {
                    break;
                }
                attempt += 1;
            }

            if !rtt_attached {
                let error_msg = format!(
                    "RTT attach failed within {}ms after {} attempt(s): {}",
                    args.rtt_timeout_ms,
                    attempt,
                    last_rtt_error.unwrap_or_else(|| "timeout expired".to_string())
                );
                status_messages.push(error_msg.clone());
                warn!("{}", error_msg);
                return Err(McpError::internal_error(
                    format!("{}\n\n{}", status_messages.join("\n"), error_msg),
                    None,
                ));
            }

            info!("RTT connected successfully, allowing channel stabilization...");
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        status_messages.push("Step 5/5: Finalizing...".to_string());
        let elapsed = start_time.elapsed();

        let message = format!(
            "Firmware deployment completed.\n\n\
            Session ID: {}\n\
            File: {}\n\
            Format: {}\n\
            Total Time: {:.1}s\n\n\
            Status:\n{}\n\n\
            Firmware is now running on target.\n\
            {}",
            args.session_id,
            file_path.display(),
            args.format,
            elapsed.as_secs_f64(),
            status_messages.join("\n"),
            if args.attach_rtt {
                "Use 'rtt_read' to monitor target output."
            } else {
                "Use 'rtt_attach' to enable real-time communication."
            }
        );

        info!(
            "Firmware deployment completed for session: {} in {:.1}s",
            args.session_id,
            elapsed.as_secs_f64()
        );
        Ok(CallToolResult::success(vec![Content::text(message)]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn flash_program_rejects_when_erase_permission_disabled() {
        let handler = EmbeddedDebuggerToolHandler::new(Config::default());
        let result = handler
            .flash_program(Parameters(FlashProgramArgs {
                session_id: "missing-session".to_string(),
                file_path: "firmware.elf".to_string(),
                format: "auto".to_string(),
                base_address: None,
                verify: true,
            }))
            .await;

        let error = format!("{:?}", result.expect_err("flash program must fail closed"));
        assert!(error.contains("Flash erase is disabled"));
    }
}
