//! MySQL connection implementation.
//!
//! This module implements the MySQL wire protocol connection,
//! including connection establishment, authentication, and state management.
//!
//! Note: The Connection trait implementation is pending. Currently provides
//! synchronous methods (query_sync, execute_sync, insert_sync) with parameter
//! binding support.

// MySQL protocol uses well-defined packet sizes that fit in u32 (max 16MB)
#![allow(clippy::cast_possible_truncation)]

use std::io::{Read, Write};
use std::net::TcpStream;

use sqlmodel_core::Error;
use sqlmodel_core::error::{
    ConnectionError, ConnectionErrorKind, ProtocolError, QueryError, QueryErrorKind,
};
use sqlmodel_core::{Row, Value};

use crate::auth;
use crate::config::MySqlConfig;
use crate::protocol::{
    Command, ErrPacket, MAX_PACKET_SIZE, PacketHeader, PacketReader, PacketType, PacketWriter,
    capabilities, charset,
};
use crate::types::{ColumnDef, FieldType, decode_text_value, interpolate_params};

/// Connection state in the MySQL protocol state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// TCP connection established, awaiting handshake
    Connecting,
    /// Performing authentication handshake
    Authenticating,
    /// Ready for queries
    Ready,
    /// Currently executing a query
    InQuery,
    /// In a transaction
    InTransaction,
    /// Connection is in an error state
    Error,
    /// Connection has been closed
    Closed,
}

/// Server capabilities received during handshake.
#[derive(Debug, Clone)]
pub struct ServerCapabilities {
    /// Server capability flags
    pub capabilities: u32,
    /// Protocol version
    pub protocol_version: u8,
    /// Server version string
    pub server_version: String,
    /// Connection ID
    pub connection_id: u32,
    /// Authentication plugin name
    pub auth_plugin: String,
    /// Authentication data (scramble)
    pub auth_data: Vec<u8>,
    /// Default charset
    pub charset: u8,
    /// Server status flags
    pub status_flags: u16,
}

/// MySQL connection.
///
/// Manages a TCP connection to a MySQL server, handling the wire protocol,
/// authentication, and state tracking.
pub struct MySqlConnection {
    /// TCP stream to the server
    stream: TcpStream,
    /// Current connection state
    state: ConnectionState,
    /// Server capabilities from handshake
    server_caps: Option<ServerCapabilities>,
    /// Connection ID
    connection_id: u32,
    /// Server status flags
    status_flags: u16,
    /// Affected rows from last statement
    affected_rows: u64,
    /// Last insert ID
    last_insert_id: u64,
    /// Number of warnings
    warnings: u16,
    /// Connection configuration
    config: MySqlConfig,
    /// Current sequence ID for packet framing
    sequence_id: u8,
}

impl std::fmt::Debug for MySqlConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MySqlConnection")
            .field("state", &self.state)
            .field("connection_id", &self.connection_id)
            .field("host", &self.config.host)
            .field("port", &self.config.port)
            .field("database", &self.config.database)
            .finish_non_exhaustive()
    }
}

impl MySqlConnection {
    /// Establish a new connection to the MySQL server.
    ///
    /// This performs the complete connection handshake:
    /// 1. TCP connection
    /// 2. Receive server handshake
    /// 3. Send handshake response with authentication
    /// 4. Handle auth result (possibly auth switch)
    #[allow(clippy::result_large_err)]
    pub fn connect(config: MySqlConfig) -> Result<Self, Error> {
        // 1. TCP connection with timeout
        let stream = TcpStream::connect_timeout(
            &config.socket_addr().parse().map_err(|e| {
                Error::Connection(ConnectionError {
                    kind: ConnectionErrorKind::Connect,
                    message: format!("Invalid socket address: {}", e),
                    source: None,
                })
            })?,
            config.connect_timeout,
        )
        .map_err(|e| {
            let kind = if e.kind() == std::io::ErrorKind::ConnectionRefused {
                ConnectionErrorKind::Refused
            } else {
                ConnectionErrorKind::Connect
            };
            Error::Connection(ConnectionError {
                kind,
                message: format!("Failed to connect to {}: {}", config.socket_addr(), e),
                source: Some(Box::new(e)),
            })
        })?;

        // Set TCP options
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(config.connect_timeout)).ok();
        stream.set_write_timeout(Some(config.connect_timeout)).ok();

        let mut conn = Self {
            stream,
            state: ConnectionState::Connecting,
            server_caps: None,
            connection_id: 0,
            status_flags: 0,
            affected_rows: 0,
            last_insert_id: 0,
            warnings: 0,
            config,
            sequence_id: 0,
        };

        // 2. Receive server handshake
        let server_caps = conn.read_handshake()?;
        conn.connection_id = server_caps.connection_id;
        conn.server_caps = Some(server_caps);
        conn.state = ConnectionState::Authenticating;

        // 3. Send handshake response
        conn.send_handshake_response()?;

        // 4. Handle authentication result
        conn.handle_auth_result()?;

        conn.state = ConnectionState::Ready;
        Ok(conn)
    }

    /// Get the current connection state.
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Check if the connection is ready for queries.
    pub fn is_ready(&self) -> bool {
        matches!(self.state, ConnectionState::Ready)
    }

    /// Get the connection ID.
    pub fn connection_id(&self) -> u32 {
        self.connection_id
    }

    /// Get the server version.
    pub fn server_version(&self) -> Option<&str> {
        self.server_caps
            .as_ref()
            .map(|caps| caps.server_version.as_str())
    }

    /// Get the number of affected rows from the last statement.
    pub fn affected_rows(&self) -> u64 {
        self.affected_rows
    }

    /// Get the last insert ID.
    pub fn last_insert_id(&self) -> u64 {
        self.last_insert_id
    }

    /// Get the number of warnings from the last statement.
    pub fn warnings(&self) -> u16 {
        self.warnings
    }

    /// Read the server handshake packet.
    #[allow(clippy::result_large_err)]
    fn read_handshake(&mut self) -> Result<ServerCapabilities, Error> {
        let (payload, _) = self.read_packet()?;
        let mut reader = PacketReader::new(&payload);

        // Protocol version
        let protocol_version = reader
            .read_u8()
            .ok_or_else(|| protocol_error("Missing protocol version"))?;

        if protocol_version != 10 {
            return Err(protocol_error(format!(
                "Unsupported protocol version: {}",
                protocol_version
            )));
        }

        // Server version (null-terminated string)
        let server_version = reader
            .read_null_string()
            .ok_or_else(|| protocol_error("Missing server version"))?;

        // Connection ID
        let connection_id = reader
            .read_u32_le()
            .ok_or_else(|| protocol_error("Missing connection ID"))?;

        // Auth plugin data part 1 (8 bytes)
        let auth_data_1 = reader
            .read_bytes(8)
            .ok_or_else(|| protocol_error("Missing auth data"))?;

        // Filler (1 byte)
        reader.skip(1);

        // Capability flags (lower 2 bytes)
        let caps_lower = reader
            .read_u16_le()
            .ok_or_else(|| protocol_error("Missing capability flags"))?;

        // Character set
        let charset = reader.read_u8().unwrap_or(charset::UTF8MB4_0900_AI_CI);

        // Status flags
        let status_flags = reader.read_u16_le().unwrap_or(0);

        // Capability flags (upper 2 bytes)
        let caps_upper = reader.read_u16_le().unwrap_or(0);
        let capabilities = u32::from(caps_lower) | (u32::from(caps_upper) << 16);

        // Length of auth-plugin-data (if CLIENT_PLUGIN_AUTH)
        let auth_data_len = if capabilities & capabilities::CLIENT_PLUGIN_AUTH != 0 {
            reader.read_u8().unwrap_or(0) as usize
        } else {
            0
        };

        // Reserved (10 bytes)
        reader.skip(10);

        // Auth plugin data part 2 (if CLIENT_SECURE_CONNECTION)
        let mut auth_data = auth_data_1.to_vec();
        if capabilities & capabilities::CLIENT_SECURE_CONNECTION != 0 {
            let len2 = if auth_data_len > 8 {
                auth_data_len - 8
            } else {
                13 // Default length
            };
            if let Some(data2) = reader.read_bytes(len2) {
                // Remove trailing NUL if present
                let data2_clean = if data2.last() == Some(&0) {
                    &data2[..data2.len() - 1]
                } else {
                    data2
                };
                auth_data.extend_from_slice(data2_clean);
            }
        }

        // Auth plugin name (if CLIENT_PLUGIN_AUTH)
        let auth_plugin = if capabilities & capabilities::CLIENT_PLUGIN_AUTH != 0 {
            reader.read_null_string().unwrap_or_default()
        } else {
            auth::plugins::MYSQL_NATIVE_PASSWORD.to_string()
        };

        Ok(ServerCapabilities {
            capabilities,
            protocol_version,
            server_version,
            connection_id,
            auth_plugin,
            auth_data,
            charset,
            status_flags,
        })
    }

    /// Send the handshake response packet.
    #[allow(clippy::result_large_err)]
    fn send_handshake_response(&mut self) -> Result<(), Error> {
        let server_caps = self
            .server_caps
            .as_ref()
            .ok_or_else(|| protocol_error("No server handshake received"))?;

        // Determine client capabilities
        let client_caps = self.config.capability_flags() & server_caps.capabilities;

        // Build authentication response
        let auth_response =
            self.compute_auth_response(&server_caps.auth_plugin, &server_caps.auth_data);

        let mut writer = PacketWriter::new();

        // Client capability flags (4 bytes)
        writer.write_u32_le(client_caps);

        // Max packet size (4 bytes)
        writer.write_u32_le(self.config.max_packet_size);

        // Character set (1 byte)
        writer.write_u8(self.config.charset);

        // Reserved (23 bytes of zeros)
        writer.write_zeros(23);

        // Username (null-terminated)
        writer.write_null_string(&self.config.user);

        // Auth response
        if client_caps & capabilities::CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA != 0 {
            writer.write_lenenc_bytes(&auth_response);
        } else if client_caps & capabilities::CLIENT_SECURE_CONNECTION != 0 {
            // Auth responses are always < 256 bytes (SHA1=20, SHA256=32)
            #[allow(clippy::cast_possible_truncation)]
            writer.write_u8(auth_response.len() as u8);
            writer.write_bytes(&auth_response);
        } else {
            writer.write_bytes(&auth_response);
            writer.write_u8(0); // Null terminator
        }

        // Database (if CLIENT_CONNECT_WITH_DB)
        if client_caps & capabilities::CLIENT_CONNECT_WITH_DB != 0 {
            if let Some(ref db) = self.config.database {
                writer.write_null_string(db);
            } else {
                writer.write_u8(0); // Empty string
            }
        }

        // Auth plugin name (if CLIENT_PLUGIN_AUTH)
        if client_caps & capabilities::CLIENT_PLUGIN_AUTH != 0 {
            writer.write_null_string(&server_caps.auth_plugin);
        }

        // Connection attributes (if CLIENT_CONNECT_ATTRS)
        if client_caps & capabilities::CLIENT_CONNECT_ATTRS != 0
            && !self.config.attributes.is_empty()
        {
            let mut attrs_writer = PacketWriter::new();
            for (key, value) in &self.config.attributes {
                attrs_writer.write_lenenc_string(key);
                attrs_writer.write_lenenc_string(value);
            }
            let attrs_data = attrs_writer.into_bytes();
            writer.write_lenenc_bytes(&attrs_data);
        }

        self.write_packet(writer.as_bytes())?;

        Ok(())
    }

    /// Compute authentication response based on the plugin.
    fn compute_auth_response(&self, plugin: &str, auth_data: &[u8]) -> Vec<u8> {
        let password = self.config.password.as_deref().unwrap_or("");

        match plugin {
            auth::plugins::MYSQL_NATIVE_PASSWORD => {
                auth::mysql_native_password(password, auth_data)
            }
            auth::plugins::CACHING_SHA2_PASSWORD => {
                auth::caching_sha2_password(password, auth_data)
            }
            auth::plugins::MYSQL_CLEAR_PASSWORD => {
                // Password + NUL terminator
                let mut result = password.as_bytes().to_vec();
                result.push(0);
                result
            }
            _ => {
                // Unknown plugin - try mysql_native_password
                auth::mysql_native_password(password, auth_data)
            }
        }
    }

    /// Handle authentication result and possible auth switch.
    #[allow(clippy::result_large_err)]
    fn handle_auth_result(&mut self) -> Result<(), Error> {
        let (payload, _) = self.read_packet()?;

        if payload.is_empty() {
            return Err(protocol_error("Empty authentication response"));
        }

        match PacketType::from_first_byte(payload[0], payload.len() as u32) {
            PacketType::Ok => {
                // Auth successful
                let mut reader = PacketReader::new(&payload);
                if let Some(ok) = reader.parse_ok_packet() {
                    self.status_flags = ok.status_flags;
                    self.affected_rows = ok.affected_rows;
                }
                Ok(())
            }
            PacketType::Error => {
                let mut reader = PacketReader::new(&payload);
                let err = reader
                    .parse_err_packet()
                    .ok_or_else(|| protocol_error("Invalid error packet"))?;
                Err(auth_error(format!(
                    "Authentication failed: {} ({})",
                    err.error_message, err.error_code
                )))
            }
            PacketType::Eof => {
                // Auth switch request - need to re-authenticate with different plugin
                self.handle_auth_switch(&payload[1..])
            }
            _ => {
                // Might be additional auth data (e.g., caching_sha2_password fast auth)
                self.handle_additional_auth(&payload)
            }
        }
    }

    /// Handle auth switch request.
    #[allow(clippy::result_large_err)]
    fn handle_auth_switch(&mut self, data: &[u8]) -> Result<(), Error> {
        let mut reader = PacketReader::new(data);

        // Plugin name
        let plugin = reader
            .read_null_string()
            .ok_or_else(|| protocol_error("Missing plugin name in auth switch"))?;

        // Auth data
        let auth_data = reader.read_rest();

        // Compute new auth response
        let response = self.compute_auth_response(&plugin, auth_data);

        // Send auth response
        self.write_packet(&response)?;

        // Read result
        self.handle_auth_result()
    }

    /// Handle additional auth data (e.g., caching_sha2_password responses).
    #[allow(clippy::result_large_err)]
    fn handle_additional_auth(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Err(protocol_error("Empty additional auth data"));
        }

        match data[0] {
            auth::caching_sha2::FAST_AUTH_SUCCESS => {
                // Fast auth succeeded, read final OK
                let (payload, _) = self.read_packet()?;
                let mut reader = PacketReader::new(&payload);
                if let Some(ok) = reader.parse_ok_packet() {
                    self.status_flags = ok.status_flags;
                }
                Ok(())
            }
            auth::caching_sha2::PERFORM_FULL_AUTH => {
                // Full auth needed - requires TLS or RSA
                // For now, return error suggesting TLS
                Err(auth_error(
                    "Full authentication required - please use TLS connection",
                ))
            }
            _ => {
                // Unknown - try to parse as OK packet
                let mut reader = PacketReader::new(data);
                if let Some(ok) = reader.parse_ok_packet() {
                    self.status_flags = ok.status_flags;
                    Ok(())
                } else {
                    Err(protocol_error(format!(
                        "Unknown auth response: {:02X}",
                        data[0]
                    )))
                }
            }
        }
    }

    /// Execute a text protocol query with parameters (synchronous).
    ///
    /// Returns a vector of rows for SELECT queries, or empty for other statements.
    /// Parameters are interpolated into the SQL string with proper escaping.
    #[allow(clippy::result_large_err)]
    pub fn query_sync(&mut self, sql: &str, params: &[Value]) -> Result<Vec<Row>, Error> {
        let sql = interpolate_params(sql, params);
        if !self.is_ready() && self.state != ConnectionState::InTransaction {
            return Err(connection_error("Connection not ready for queries"));
        }

        self.state = ConnectionState::InQuery;
        self.sequence_id = 0;

        // Send COM_QUERY
        let mut writer = PacketWriter::new();
        writer.write_u8(Command::Query as u8);
        writer.write_bytes(sql.as_bytes());
        self.write_packet(writer.as_bytes())?;

        // Read response
        let (payload, _) = self.read_packet()?;

        if payload.is_empty() {
            self.state = ConnectionState::Ready;
            return Err(protocol_error("Empty query response"));
        }

        match PacketType::from_first_byte(payload[0], payload.len() as u32) {
            PacketType::Ok => {
                // Non-result statement (INSERT, UPDATE, DELETE, etc.)
                let mut reader = PacketReader::new(&payload);
                if let Some(ok) = reader.parse_ok_packet() {
                    self.affected_rows = ok.affected_rows;
                    self.last_insert_id = ok.last_insert_id;
                    self.status_flags = ok.status_flags;
                    self.warnings = ok.warnings;
                }
                self.state = if self.status_flags
                    & crate::protocol::server_status::SERVER_STATUS_IN_TRANS
                    != 0
                {
                    ConnectionState::InTransaction
                } else {
                    ConnectionState::Ready
                };
                Ok(vec![])
            }
            PacketType::Error => {
                self.state = ConnectionState::Ready;
                let mut reader = PacketReader::new(&payload);
                let err = reader
                    .parse_err_packet()
                    .ok_or_else(|| protocol_error("Invalid error packet"))?;
                Err(query_error(&err))
            }
            PacketType::LocalInfile => {
                self.state = ConnectionState::Ready;
                Err(query_error_msg("LOCAL INFILE not supported"))
            }
            _ => {
                // Result set - first byte is column count
                self.read_result_set(&payload)
            }
        }
    }

    /// Read a result set (column definitions and rows).
    #[allow(clippy::result_large_err)]
    fn read_result_set(&mut self, first_packet: &[u8]) -> Result<Vec<Row>, Error> {
        let mut reader = PacketReader::new(first_packet);
        let column_count = reader
            .read_lenenc_int()
            .ok_or_else(|| protocol_error("Invalid column count"))?
            as usize;

        // Read column definitions
        let mut columns = Vec::with_capacity(column_count);
        for _ in 0..column_count {
            let (payload, _) = self.read_packet()?;
            columns.push(self.parse_column_def(&payload)?);
        }

        // Check for EOF packet (if not CLIENT_DEPRECATE_EOF)
        let server_caps = self
            .server_caps
            .as_ref()
            .map_or(0, |c| c.capabilities);
        if server_caps & capabilities::CLIENT_DEPRECATE_EOF == 0 {
            let (payload, _) = self.read_packet()?;
            if payload.first() == Some(&0xFE) {
                // EOF packet - continue to rows
            }
        }

        // Read rows until EOF or OK
        let mut rows = Vec::new();
        loop {
            let (payload, _) = self.read_packet()?;

            if payload.is_empty() {
                break;
            }

            match PacketType::from_first_byte(payload[0], payload.len() as u32) {
                PacketType::Eof | PacketType::Ok => {
                    // End of result set
                    let mut reader = PacketReader::new(&payload);
                    if payload[0] == 0x00 {
                        if let Some(ok) = reader.parse_ok_packet() {
                            self.status_flags = ok.status_flags;
                            self.warnings = ok.warnings;
                        }
                    } else if payload[0] == 0xFE {
                        if let Some(eof) = reader.parse_eof_packet() {
                            self.status_flags = eof.status_flags;
                            self.warnings = eof.warnings;
                        }
                    }
                    break;
                }
                PacketType::Error => {
                    let mut reader = PacketReader::new(&payload);
                    let err = reader
                        .parse_err_packet()
                        .ok_or_else(|| protocol_error("Invalid error packet"))?;
                    self.state = ConnectionState::Ready;
                    return Err(query_error(&err));
                }
                _ => {
                    // Data row
                    let row = self.parse_text_row(&payload, &columns);
                    rows.push(row);
                }
            }
        }

        self.state =
            if self.status_flags & crate::protocol::server_status::SERVER_STATUS_IN_TRANS != 0 {
                ConnectionState::InTransaction
            } else {
                ConnectionState::Ready
            };

        Ok(rows)
    }

    /// Parse a column definition packet.
    #[allow(clippy::result_large_err)]
    fn parse_column_def(&self, data: &[u8]) -> Result<ColumnDef, Error> {
        let mut reader = PacketReader::new(data);

        let catalog = reader
            .read_lenenc_string()
            .ok_or_else(|| protocol_error("Missing catalog"))?;
        let schema = reader
            .read_lenenc_string()
            .ok_or_else(|| protocol_error("Missing schema"))?;
        let table = reader
            .read_lenenc_string()
            .ok_or_else(|| protocol_error("Missing table"))?;
        let org_table = reader
            .read_lenenc_string()
            .ok_or_else(|| protocol_error("Missing org_table"))?;
        let name = reader
            .read_lenenc_string()
            .ok_or_else(|| protocol_error("Missing name"))?;
        let org_name = reader
            .read_lenenc_string()
            .ok_or_else(|| protocol_error("Missing org_name"))?;

        // Length of fixed fields
        let _fixed_len = reader.read_lenenc_int();

        let charset = reader
            .read_u16_le()
            .ok_or_else(|| protocol_error("Missing charset"))?;
        let column_length = reader
            .read_u32_le()
            .ok_or_else(|| protocol_error("Missing column_length"))?;
        let column_type = FieldType::from_u8(
            reader
                .read_u8()
                .ok_or_else(|| protocol_error("Missing column_type"))?,
        );
        let flags = reader
            .read_u16_le()
            .ok_or_else(|| protocol_error("Missing flags"))?;
        let decimals = reader
            .read_u8()
            .ok_or_else(|| protocol_error("Missing decimals"))?;

        Ok(ColumnDef {
            catalog,
            schema,
            table,
            org_table,
            name,
            org_name,
            charset,
            column_length,
            column_type,
            flags,
            decimals,
        })
    }

    /// Parse a text protocol row.
    fn parse_text_row(&self, data: &[u8], columns: &[ColumnDef]) -> Row {
        let mut reader = PacketReader::new(data);
        let mut values = Vec::with_capacity(columns.len());

        for col in columns {
            // In text protocol, each value is a length-encoded string
            // 0xFB indicates NULL
            if reader.peek() == Some(0xFB) {
                reader.skip(1);
                values.push(Value::Null);
            } else if let Some(data) = reader.read_lenenc_bytes() {
                let is_unsigned = col.is_unsigned();
                let value = decode_text_value(col.column_type, &data, is_unsigned);
                values.push(value);
            } else {
                values.push(Value::Null);
            }
        }

        // Build column names for the Row
        let column_names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();

        Row::new(column_names, values)
    }

    /// Execute a query and return the first row, if any (synchronous).
    #[allow(clippy::result_large_err)]
    pub fn query_one_sync(&mut self, sql: &str, params: &[Value]) -> Result<Option<Row>, Error> {
        let rows = self.query_sync(sql, params)?;
        Ok(rows.into_iter().next())
    }

    /// Execute a statement that doesn't return rows (synchronous).
    #[allow(clippy::result_large_err)]
    pub fn execute_sync(&mut self, sql: &str, params: &[Value]) -> Result<u64, Error> {
        self.query_sync(sql, params)?;
        Ok(self.affected_rows)
    }

    /// Execute an INSERT and return the last inserted ID (synchronous).
    #[allow(clippy::result_large_err)]
    pub fn insert_sync(&mut self, sql: &str, params: &[Value]) -> Result<i64, Error> {
        self.query_sync(sql, params)?;
        Ok(self.last_insert_id as i64)
    }

    /// Ping the server to check connection.
    #[allow(clippy::result_large_err)]
    pub fn ping(&mut self) -> Result<(), Error> {
        self.sequence_id = 0;

        let mut writer = PacketWriter::new();
        writer.write_u8(Command::Ping as u8);
        self.write_packet(writer.as_bytes())?;

        let (payload, _) = self.read_packet()?;

        if payload.first() == Some(&0x00) {
            Ok(())
        } else {
            Err(connection_error("Ping failed"))
        }
    }

    /// Close the connection gracefully.
    #[allow(clippy::result_large_err)]
    pub fn close(mut self) -> Result<(), Error> {
        if self.state == ConnectionState::Closed {
            return Ok(());
        }

        self.sequence_id = 0;

        let mut writer = PacketWriter::new();
        writer.write_u8(Command::Quit as u8);

        // Best effort - ignore errors on close
        let _ = self.write_packet(writer.as_bytes());

        self.state = ConnectionState::Closed;
        Ok(())
    }

    /// Read a complete packet from the stream.
    #[allow(clippy::result_large_err)]
    fn read_packet(&mut self) -> Result<(Vec<u8>, u8), Error> {
        // Read header (4 bytes)
        let mut header_buf = [0u8; 4];
        self.stream.read_exact(&mut header_buf).map_err(|e| {
            Error::Connection(ConnectionError {
                kind: ConnectionErrorKind::Disconnected,
                message: format!("Failed to read packet header: {}", e),
                source: Some(Box::new(e)),
            })
        })?;

        let header = PacketHeader::from_bytes(&header_buf);
        let payload_len = header.payload_length as usize;
        self.sequence_id = header.sequence_id.wrapping_add(1);

        // Read payload
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            self.stream.read_exact(&mut payload).map_err(|e| {
                Error::Connection(ConnectionError {
                    kind: ConnectionErrorKind::Disconnected,
                    message: format!("Failed to read packet payload: {}", e),
                    source: Some(Box::new(e)),
                })
            })?;
        }

        // Handle multi-packet payloads
        if payload_len == MAX_PACKET_SIZE {
            loop {
                let mut header_buf = [0u8; 4];
                self.stream.read_exact(&mut header_buf).map_err(|e| {
                    Error::Connection(ConnectionError {
                        kind: ConnectionErrorKind::Disconnected,
                        message: format!("Failed to read continuation header: {}", e),
                        source: Some(Box::new(e)),
                    })
                })?;

                let cont_header = PacketHeader::from_bytes(&header_buf);
                let cont_len = cont_header.payload_length as usize;
                self.sequence_id = cont_header.sequence_id.wrapping_add(1);

                if cont_len > 0 {
                    let mut cont_payload = vec![0u8; cont_len];
                    self.stream.read_exact(&mut cont_payload).map_err(|e| {
                        Error::Connection(ConnectionError {
                            kind: ConnectionErrorKind::Disconnected,
                            message: format!("Failed to read continuation payload: {}", e),
                            source: Some(Box::new(e)),
                        })
                    })?;
                    payload.extend_from_slice(&cont_payload);
                }

                if cont_len < MAX_PACKET_SIZE {
                    break;
                }
            }
        }

        Ok((payload, header.sequence_id))
    }

    /// Write a packet to the stream.
    #[allow(clippy::result_large_err)]
    fn write_packet(&mut self, payload: &[u8]) -> Result<(), Error> {
        let writer = PacketWriter::new();
        let packet = writer.build_packet_from_payload(payload, self.sequence_id);
        self.sequence_id = self.sequence_id.wrapping_add(1);

        self.stream.write_all(&packet).map_err(|e| {
            Error::Connection(ConnectionError {
                kind: ConnectionErrorKind::Disconnected,
                message: format!("Failed to write packet: {}", e),
                source: Some(Box::new(e)),
            })
        })?;

        self.stream.flush().map_err(|e| {
            Error::Connection(ConnectionError {
                kind: ConnectionErrorKind::Disconnected,
                message: format!("Failed to flush stream: {}", e),
                source: Some(Box::new(e)),
            })
        })?;

        Ok(())
    }
}

// Helper functions for creating errors

fn protocol_error(msg: impl Into<String>) -> Error {
    Error::Protocol(ProtocolError {
        message: msg.into(),
        raw_data: None,
        source: None,
    })
}

fn auth_error(msg: impl Into<String>) -> Error {
    Error::Connection(ConnectionError {
        kind: ConnectionErrorKind::Authentication,
        message: msg.into(),
        source: None,
    })
}

fn connection_error(msg: impl Into<String>) -> Error {
    Error::Connection(ConnectionError {
        kind: ConnectionErrorKind::Connect,
        message: msg.into(),
        source: None,
    })
}

fn query_error(err: &ErrPacket) -> Error {
    let kind = if err.is_duplicate_key() || err.is_foreign_key_violation() {
        QueryErrorKind::Constraint
    } else {
        QueryErrorKind::Syntax
    };

    Error::Query(QueryError {
        kind,
        message: err.error_message.clone(),
        sqlstate: Some(err.sql_state.clone()),
        sql: None,
        detail: None,
        hint: None,
        position: None,
        source: None,
    })
}

fn query_error_msg(msg: impl Into<String>) -> Error {
    Error::Query(QueryError {
        kind: QueryErrorKind::Syntax,
        message: msg.into(),
        sqlstate: None,
        sql: None,
        detail: None,
        hint: None,
        position: None,
        source: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_default() {
        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
    }

    #[test]
    fn test_error_helpers() {
        let err = protocol_error("test error");
        assert!(matches!(err, Error::Protocol(_)));

        let err = auth_error("auth failed");
        assert!(matches!(err, Error::Connection(_)));

        let err = connection_error("connection failed");
        assert!(matches!(err, Error::Connection(_)));
    }

    #[test]
    fn test_query_error_duplicate_key() {
        let err_packet = ErrPacket {
            error_code: 1062,
            sql_state: "23000".to_string(),
            error_message: "Duplicate entry".to_string(),
        };

        let err = query_error(&err_packet);
        if let Error::Query(q) = err {
            assert_eq!(q.kind, QueryErrorKind::Constraint);
        } else {
            panic!("Expected query error");
        }
    }
}
