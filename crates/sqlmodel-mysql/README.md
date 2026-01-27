# sqlmodel-mysql

MySQL wire protocol driver for SQLModel Rust.

## Status

**Current State**: Synchronous implementation complete, async conversion pending.

### What Works

- TCP connection establishment
- Protocol handshake (v10)
- Authentication (mysql_native_password, caching_sha2_password)
- Text protocol queries with parameter binding
- Error handling with MySQL error codes
- Connection state machine
- Type encoding/decoding for common types
- 58 unit tests passing

### What's Missing

- [ ] **Async conversion** - Convert from `std::net::TcpStream` to `asupersync::net::TcpStream`
- [ ] **Connection trait** - Implement `sqlmodel_core::Connection` trait
- [ ] **Prepared statements** - Binary protocol (COM_STMT_PREPARE, COM_STMT_EXECUTE)
- [ ] **SSL/TLS** - Encrypted connections
- [ ] **Integration tests** - Tests against real MySQL database

## Current API (Synchronous)

```rust
use sqlmodel_mysql::{MySqlConfig, MySqlConnection};
use sqlmodel_core::Value;

// Connect
let config = MySqlConfig::new()
    .host("localhost")
    .port(3306)
    .user("root")
    .password("secret")
    .database("mydb");

let mut conn = MySqlConnection::connect(config)?;

// Query with parameters
let rows = conn.query_sync(
    "SELECT * FROM users WHERE id = ?",
    &[Value::Int(1)]
)?;

// Execute statement
let affected = conn.execute_sync(
    "UPDATE users SET name = ? WHERE id = ?",
    &[Value::Text("Alice".into()), Value::Int(1)]
)?;

// Insert and get last ID
let id = conn.insert_sync(
    "INSERT INTO users (name) VALUES (?)",
    &[Value::Text("Bob".into())]
)?;

// Ping to check connection
conn.ping()?;

// Close gracefully
conn.close()?;
```

## Migration to Async

The async migration requires:

1. **Replace TcpStream**: Use `asupersync::net::TcpStream` instead of `std::net::TcpStream`
2. **Async I/O**: Convert `read_packet`/`write_packet` to async with `.await`
3. **Add Cx context**: All async methods take `&Cx` for cancellation support
4. **Return Outcome**: Use `Outcome<T, E>` instead of `Result<T, E>`
5. **Implement Connection trait**: Match the `sqlmodel_core::Connection` interface

Example target API:

```rust
use sqlmodel_core::Connection;
use asupersync::Cx;

// With async Connection trait
let rows = conn.query(&cx, "SELECT * FROM users WHERE id = ?", &[Value::Int(1)]).await?;
let mut tx = conn.begin(&cx).await?;
tx.execute(&cx, "INSERT INTO logs (msg) VALUES (?)", &[Value::Text("action".into())]).await?;
tx.commit(&cx).await?;
```

## Type Mapping

| MySQL Type | Rust Type |
|------------|-----------|
| TINYINT | `i8` |
| SMALLINT | `i16` |
| INT | `i32` |
| BIGINT | `i64` |
| FLOAT | `f32` |
| DOUBLE | `f64` |
| VARCHAR, TEXT | `String` |
| BLOB | `Vec<u8>` |
| DATE | Date value |
| DATETIME | Timestamp value |
| JSON | `serde_json::Value` |

## References

- [MySQL Protocol Documentation](https://dev.mysql.com/doc/dev/mysql-server/latest/PAGE_PROTOCOL.html)
- [MariaDB Protocol](https://mariadb.com/kb/en/clientserver-protocol/)
