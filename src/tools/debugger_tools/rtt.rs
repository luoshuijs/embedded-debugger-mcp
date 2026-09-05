use rmcp::model::ContentBlock as Content;
use rmcp::{
    handler::server::wrapper::Parameters, model::*, tool, tool_router, ErrorData as McpError,
};
use tracing::{debug, error, info};

use super::formatting::parse_address;
use super::session::EmbeddedDebuggerToolHandler;
use crate::tools::types::*;

#[tool_router(router = rtt_tool_router, vis = "pub")]
impl EmbeddedDebuggerToolHandler {
    // =============================================================================
    // RTT Communication Tools (5 tools)
    // =============================================================================

    #[tool(description = "Attach to RTT (Real-Time Transfer) for communication with target")]
    async fn rtt_attach(
        &self,
        Parameters(args): Parameters<RttAttachArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Attaching RTT for session: {}", args.session_id);

        let session_arc = self.get_session(&args.session_id).await?;

        // Parse control block address if provided
        let control_block_address = if let Some(addr_str) = args.control_block_address.as_deref() {
            match parse_address(addr_str) {
                Ok(addr) => Some(addr),
                Err(e) => {
                    let error_msg = format!("Invalid control block address '{}': {}", addr_str, e);
                    return Err(McpError::internal_error(error_msg, None));
                }
            }
        } else {
            None
        };

        // Parse memory ranges if provided
        let memory_ranges = if let Some(ranges) = args.memory_ranges {
            let mut parsed_ranges = Vec::new();
            for range in ranges {
                let start = parse_address(&range.start).map_err(|e| {
                    McpError::internal_error(
                        format!("Invalid start address '{}': {}", range.start, e),
                        None,
                    )
                })?;
                let end = parse_address(&range.end).map_err(|e| {
                    McpError::internal_error(
                        format!("Invalid end address '{}': {}", range.end, e),
                        None,
                    )
                })?;
                parsed_ranges.push((start, end));
            }
            Some(parsed_ranges)
        } else {
            None
        };

        let (control_block_address, memory_ranges) = self.prepare_rtt_scan_region(
            &session_arc.target_chip,
            control_block_address,
            memory_ranges,
        )?;

        // Attach RTT
        {
            let mut rtt_manager = session_arc.rtt_manager.lock().await;
            match rtt_manager
                .attach(
                    session_arc.probe_session()?,
                    control_block_address,
                    memory_ranges,
                )
                .await
            {
                Ok(_) => {
                    let up_channels = rtt_manager.up_channel_count();
                    let down_channels = rtt_manager.down_channel_count();

                    let message = format!(
                        "RTT attached successfully!\n\n\
                        Session ID: {}\n\
                        Up Channels (Target to Host): {}\n\
                        Down Channels (Host to Target): {}\n\n\
                        RTT is now ready for real-time communication with the target.\n\
                        Use 'rtt_read' to read from target and 'rtt_write' to send data to target.",
                        args.session_id, up_channels, down_channels
                    );

                    info!("RTT attached successfully for session: {}", args.session_id);
                    Ok(CallToolResult::success(vec![Content::text(message)]))
                }
                Err(e) => {
                    error!(
                        "Failed to attach RTT for session {}: {}",
                        args.session_id, e
                    );
                    let error_msg = format!(
                        "Failed to attach RTT\n\n\
                        Session ID: {}\n\
                        Error: {}\n\n\
                        Suggestions:\n\
                        - Ensure the target firmware has RTT enabled and initialized\n\
                        - Check that the target is halted\n\
                        - Verify memory ranges if specified\n\
                        - Try different control block address if known",
                        args.session_id, e
                    );
                    Err(McpError::internal_error(error_msg, None))
                }
            }
        }
    }

    #[tool(description = "Detach from RTT communication")]
    async fn rtt_detach(
        &self,
        Parameters(args): Parameters<RttDetachArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Detaching RTT for session: {}", args.session_id);

        let session_arc = self.get_session(&args.session_id).await?;

        // Detach RTT
        {
            let mut rtt_manager = session_arc.rtt_manager.lock().await;
            match rtt_manager.detach().await {
                Ok(_) => {
                    let message = format!(
                        "RTT detached successfully\n\n\
                        Session ID: {}\n\n\
                        RTT communication has been closed.",
                        args.session_id
                    );

                    info!("RTT detached successfully for session: {}", args.session_id);
                    Ok(CallToolResult::success(vec![Content::text(message)]))
                }
                Err(e) => {
                    error!(
                        "Failed to detach RTT for session {}: {}",
                        args.session_id, e
                    );
                    let error_msg = format!("Failed to detach RTT: {}", e);
                    Err(McpError::internal_error(error_msg, None))
                }
            }
        }
    }

    #[tool(description = "Read data from RTT up channel (target to host)")]
    async fn rtt_read(
        &self,
        Parameters(args): Parameters<RttReadArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Reading from RTT channel {} for session: {}",
            args.channel, args.session_id
        );

        let session_arc = self.get_session(&args.session_id).await?;
        if args.max_bytes == 0 {
            return Err(McpError::internal_error(
                "max_bytes must be greater than zero.".to_string(),
                None,
            ));
        }
        if args.max_bytes > self.config.rtt.buffer_size {
            return Err(McpError::internal_error(
                format!(
                    "max_bytes {} exceeds configured RTT buffer size {}.",
                    args.max_bytes, self.config.rtt.buffer_size
                ),
                None,
            ));
        }

        // Read from RTT
        {
            let mut rtt_manager = session_arc.rtt_manager.lock().await;
            if !rtt_manager.is_attached() {
                let error_msg = format!(
                    "RTT not attached for session '{}'\n\nUse 'rtt_attach' first",
                    args.session_id
                );
                return Err(McpError::internal_error(error_msg, None));
            }

            match rtt_manager
                .read_channel(args.channel, args.max_bytes, args.timeout_ms)
                .await
            {
                Ok(data) => {
                    let data_len = data.len();
                    let data_str = if data.is_empty() {
                        "No data available".to_string()
                    } else {
                        // Try to decode as UTF-8, fall back to hex if not valid
                        match String::from_utf8(data.clone()) {
                            Ok(text) => {
                                if text
                                    .chars()
                                    .all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
                                {
                                    format!("Text: {}", text)
                                } else {
                                    format!("Mixed: {} (hex: {})", text, hex::encode(&data))
                                }
                            }
                            Err(_) => format!("Binary data (hex): {}", hex::encode(&data)),
                        }
                    };

                    let message = format!(
                        "RTT Read from Channel {}\n\n\
                        Session ID: {}\n\
                        Bytes Read: {}\n\n\
                        Data:\n{}",
                        args.channel, args.session_id, data_len, data_str
                    );

                    debug!(
                        "Read {} bytes from RTT channel {} for session: {}",
                        data_len, args.channel, args.session_id
                    );
                    Ok(CallToolResult::success(vec![Content::text(message)]))
                }
                Err(e) => {
                    error!(
                        "Failed to read from RTT channel {} for session {}: {}",
                        args.channel, args.session_id, e
                    );
                    let error_msg = format!(
                        "Failed to read from RTT channel {}\n\n\
                        Session ID: {}\n\
                        Error: {}",
                        args.channel, args.session_id, e
                    );
                    Err(McpError::internal_error(error_msg, None))
                }
            }
        }
    }

    #[tool(description = "Write data to RTT down channel (host to target)")]
    async fn rtt_write(
        &self,
        Parameters(args): Parameters<RttWriteArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Writing to RTT channel {} for session: {}",
            args.channel, args.session_id
        );

        // Get session from storage
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

        // Parse data based on encoding
        let data_bytes = match args.encoding.as_str() {
            "utf8" => args.data.as_bytes().to_vec(),
            "hex" => match hex::decode(&args.data) {
                Ok(bytes) => bytes,
                Err(e) => {
                    let error_msg = format!("Invalid hex data '{}': {}", args.data, e);
                    return Err(McpError::internal_error(error_msg, None));
                }
            },
            "binary" => {
                // Parse binary string like "10110011 11001100"
                let binary_str = args.data.replace(' ', "");
                if binary_str.len() % 8 != 0 {
                    let error_msg =
                        format!("Binary data must be multiple of 8 bits: '{}'", args.data);
                    return Err(McpError::internal_error(error_msg, None));
                }

                let mut bytes = Vec::new();
                for chunk in binary_str.chars().collect::<Vec<_>>().chunks(8) {
                    let byte_str: String = chunk.iter().copied().collect();
                    match u8::from_str_radix(&byte_str, 2) {
                        Ok(byte) => bytes.push(byte),
                        Err(e) => {
                            let error_msg = format!("Invalid binary byte '{}': {}", byte_str, e);
                            return Err(McpError::internal_error(error_msg, None));
                        }
                    }
                }
                bytes
            }
            _ => {
                let error_msg = format!(
                    "Unsupported encoding '{}'. Use 'utf8', 'hex', or 'binary'",
                    args.encoding
                );
                return Err(McpError::internal_error(error_msg, None));
            }
        };

        // Write to RTT
        {
            let mut rtt_manager = session_arc.rtt_manager.lock().await;
            if !rtt_manager.is_attached() {
                let error_msg = format!(
                    "RTT not attached for session '{}'\n\nUse 'rtt_attach' first",
                    args.session_id
                );
                return Err(McpError::internal_error(error_msg, None));
            }

            match rtt_manager.write_channel(args.channel, &data_bytes).await {
                Ok(bytes_written) => {
                    let message = format!(
                        "RTT Write to Channel {}\n\n\
                        Session ID: {}\n\
                        Data: {}\n\
                        Encoding: {}\n\
                        Bytes Written: {}\n\n\
                        Data sent successfully to target.",
                        args.channel, args.session_id, args.data, args.encoding, bytes_written
                    );

                    info!(
                        "Wrote {} bytes to RTT channel {} for session: {}",
                        bytes_written, args.channel, args.session_id
                    );
                    Ok(CallToolResult::success(vec![Content::text(message)]))
                }
                Err(e) => {
                    error!(
                        "Failed to write to RTT channel {} for session {}: {}",
                        args.channel, args.session_id, e
                    );
                    let error_msg = format!(
                        "Failed to write to RTT channel {}\n\n\
                        Session ID: {}\n\
                        Error: {}",
                        args.channel, args.session_id, e
                    );
                    Err(McpError::internal_error(error_msg, None))
                }
            }
        }
    }

    #[tool(description = "List available RTT channels")]
    async fn rtt_channels(
        &self,
        Parameters(args): Parameters<RttChannelsArgs>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Listing RTT channels for session: {}", args.session_id);

        // Get session from storage
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

        // List RTT channels
        {
            let rtt_manager = session_arc.rtt_manager.lock().await;
            if !rtt_manager.is_attached() {
                let error_msg = format!(
                    "RTT not attached for session '{}'\n\nUse 'rtt_attach' first",
                    args.session_id
                );
                return Err(McpError::internal_error(error_msg, None));
            }

            let channels = rtt_manager.get_channels();
            let channel_count = channels.len();

            if channels.is_empty() {
                let message = format!(
                    "RTT Channels\n\n\
                    Session ID: {}\n\n\
                    No RTT channels available.",
                    args.session_id
                );
                return Ok(CallToolResult::success(vec![Content::text(message)]));
            }

            let mut message = format!("RTT Channels\n\nSession ID: {}\n\n", args.session_id);

            // Group channels by direction
            let mut up_channels = Vec::new();
            let mut down_channels = Vec::new();

            for channel in &channels {
                match channel.direction {
                    crate::rtt::ChannelDirection::Up => up_channels.push(channel),
                    crate::rtt::ChannelDirection::Down => down_channels.push(channel),
                }
            }

            if !up_channels.is_empty() {
                message.push_str("Up Channels (Target to Host):\n");
                for channel in up_channels {
                    message.push_str(&format!(
                        "  {}. {} (Size: {} bytes, Mode: {})\n",
                        channel.id, channel.name, channel.buffer_size, channel.mode
                    ));
                }
                message.push('\n');
            }

            if !down_channels.is_empty() {
                message.push_str("Down Channels (Host to Target):\n");
                for channel in down_channels {
                    message.push_str(&format!(
                        "  {}. {} (Size: {} bytes, Mode: {})\n",
                        channel.id, channel.name, channel.buffer_size, channel.mode
                    ));
                }
            }

            info!(
                "Listed {} RTT channels for session: {}",
                channel_count, args.session_id
            );
            Ok(CallToolResult::success(vec![Content::text(message)]))
        }
    }
}
