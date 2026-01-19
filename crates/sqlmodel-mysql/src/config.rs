//! MySQL connection configuration.
//!
//! Provides connection parameters for establishing MySQL connections
//! including authentication, SSL, and connection options.

use std::collections::HashMap;
use std::time::Duration;

/// SSL mode for MySQL connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SslMode {
    /// Do not use SSL
    #[default]
    Disable,
    /// Prefer SSL if available, fall back to non-SSL
    Preferred,
    /// Require SSL connection
    Required,
    /// Require SSL and verify server certificate
    VerifyCa,
    /// Require SSL and verify server certificate matches hostname
    VerifyIdentity,
}

impl SslMode {
    /// Check if SSL should be attempted.
    pub const fn should_try_ssl(self) -> bool {
        !matches!(self, SslMode::Disable)
    }

    /// Check if SSL is required.
    pub const fn is_required(self) -> bool {
        matches!(
            self,
            SslMode::Required | SslMode::VerifyCa | SslMode::VerifyIdentity
        )
    }
}

/// MySQL connection configuration.
#[derive(Debug, Clone)]
pub struct MySqlConfig {
    /// Hostname or IP address
    pub host: String,
    /// Port number (default: 3306)
    pub port: u16,
    /// Username for authentication
    pub user: String,
    /// Password for authentication
    pub password: Option<String>,
    /// Database name to connect to (optional at connect time)
    pub database: Option<String>,
    /// Character set (default: utf8mb4)
    pub charset: u8,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// SSL mode
    pub ssl_mode: SslMode,
    /// Enable compression (CLIENT_COMPRESS capability)
    pub compression: bool,
    /// Additional connection attributes
    pub attributes: HashMap<String, String>,
    /// Local infile handling (disabled by default for security)
    pub local_infile: bool,
    /// Max allowed packet size (default: 64MB)
    pub max_packet_size: u32,
}

impl Default for MySqlConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3306,
            user: String::new(),
            password: None,
            database: None,
            charset: crate::protocol::charset::UTF8MB4_0900_AI_CI,
            connect_timeout: Duration::from_secs(30),
            ssl_mode: SslMode::default(),
            compression: false,
            attributes: HashMap::new(),
            local_infile: false,
            max_packet_size: 64 * 1024 * 1024, // 64MB
        }
    }
}

impl MySqlConfig {
    /// Create a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the hostname.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Set the port.
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the username.
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    /// Set the password.
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Set the database.
    pub fn database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Set the character set.
    pub fn charset(mut self, charset: u8) -> Self {
        self.charset = charset;
        self
    }

    /// Set the connection timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Set the SSL mode.
    pub fn ssl_mode(mut self, mode: SslMode) -> Self {
        self.ssl_mode = mode;
        self
    }

    /// Enable or disable compression.
    pub fn compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }

    /// Set a connection attribute.
    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Enable or disable local infile handling.
    ///
    /// # Security Warning
    /// Enabling local infile can be a security risk. Only enable if you
    /// trust the server and understand the implications.
    pub fn local_infile(mut self, enabled: bool) -> Self {
        self.local_infile = enabled;
        self
    }

    /// Set the max allowed packet size.
    pub fn max_packet_size(mut self, size: u32) -> Self {
        self.max_packet_size = size;
        self
    }

    /// Get the socket address string for connection.
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Build capability flags based on configuration.
    pub fn capability_flags(&self) -> u32 {
        use crate::protocol::capabilities::{
            CLIENT_COMPRESS, CLIENT_CONNECT_ATTRS, CLIENT_CONNECT_WITH_DB, CLIENT_LOCAL_FILES,
            CLIENT_SSL, DEFAULT_CLIENT_FLAGS,
        };

        let mut flags = DEFAULT_CLIENT_FLAGS;

        if self.database.is_some() {
            flags |= CLIENT_CONNECT_WITH_DB;
        }

        if self.ssl_mode.should_try_ssl() {
            flags |= CLIENT_SSL;
        }

        if self.compression {
            flags |= CLIENT_COMPRESS;
        }

        if self.local_infile {
            flags |= CLIENT_LOCAL_FILES;
        }

        if !self.attributes.is_empty() {
            flags |= CLIENT_CONNECT_ATTRS;
        }

        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = MySqlConfig::new()
            .host("db.example.com")
            .port(3307)
            .user("myuser")
            .password("secret")
            .database("testdb")
            .connect_timeout(Duration::from_secs(10))
            .ssl_mode(SslMode::Required)
            .compression(true)
            .attribute("program_name", "myapp");

        assert_eq!(config.host, "db.example.com");
        assert_eq!(config.port, 3307);
        assert_eq!(config.user, "myuser");
        assert_eq!(config.password, Some("secret".to_string()));
        assert_eq!(config.database, Some("testdb".to_string()));
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.ssl_mode, SslMode::Required);
        assert!(config.compression);
        assert_eq!(
            config.attributes.get("program_name"),
            Some(&"myapp".to_string())
        );
    }

    #[test]
    fn test_socket_addr() {
        let config = MySqlConfig::new().host("db.example.com").port(3307);
        assert_eq!(config.socket_addr(), "db.example.com:3307");
    }

    #[test]
    fn test_ssl_mode_properties() {
        assert!(!SslMode::Disable.should_try_ssl());
        assert!(!SslMode::Disable.is_required());

        assert!(SslMode::Preferred.should_try_ssl());
        assert!(!SslMode::Preferred.is_required());

        assert!(SslMode::Required.should_try_ssl());
        assert!(SslMode::Required.is_required());

        assert!(SslMode::VerifyCa.should_try_ssl());
        assert!(SslMode::VerifyCa.is_required());

        assert!(SslMode::VerifyIdentity.should_try_ssl());
        assert!(SslMode::VerifyIdentity.is_required());
    }

    #[test]
    fn test_capability_flags() {
        use crate::protocol::capabilities::*;

        let config = MySqlConfig::new().database("test").compression(true);
        let flags = config.capability_flags();

        assert!(flags & CLIENT_CONNECT_WITH_DB != 0);
        assert!(flags & CLIENT_COMPRESS != 0);
        assert!(flags & CLIENT_PROTOCOL_41 != 0);
        assert!(flags & CLIENT_SECURE_CONNECTION != 0);
    }

    #[test]
    fn test_default_config() {
        let config = MySqlConfig::default();

        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 3306);
        assert_eq!(config.ssl_mode, SslMode::Disable);
        assert!(!config.compression);
        assert!(!config.local_infile);
    }
}
