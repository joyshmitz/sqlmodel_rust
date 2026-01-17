//! Database introspection.

use asupersync::{Cx, Outcome};
use sqlmodel_core::{Connection, Error};

/// Information about a database table.
#[derive(Debug, Clone)]
pub struct TableInfo {
    /// Table name
    pub name: String,
    /// Columns in the table
    pub columns: Vec<ColumnInfo>,
    /// Primary key column names
    pub primary_key: Vec<String>,
    /// Foreign key constraints
    pub foreign_keys: Vec<ForeignKeyInfo>,
    /// Indexes on the table
    pub indexes: Vec<IndexInfo>,
}

/// Information about a table column.
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Column name
    pub name: String,
    /// SQL type
    pub sql_type: String,
    /// Whether the column is nullable
    pub nullable: bool,
    /// Default value expression
    pub default: Option<String>,
    /// Whether this is part of the primary key
    pub primary_key: bool,
    /// Whether this column auto-increments
    pub auto_increment: bool,
}

/// Information about a foreign key constraint.
#[derive(Debug, Clone)]
pub struct ForeignKeyInfo {
    /// Constraint name
    pub name: Option<String>,
    /// Local column name
    pub column: String,
    /// Referenced table
    pub foreign_table: String,
    /// Referenced column
    pub foreign_column: String,
    /// ON DELETE action
    pub on_delete: Option<String>,
    /// ON UPDATE action
    pub on_update: Option<String>,
}

/// Information about an index.
#[derive(Debug, Clone)]
pub struct IndexInfo {
    /// Index name
    pub name: String,
    /// Columns in the index
    pub columns: Vec<String>,
    /// Whether this is a unique index
    pub unique: bool,
}

/// Database introspector.
pub struct Introspector {
    /// Database type for dialect-specific queries
    dialect: Dialect,
}

/// Supported database dialects.
#[derive(Debug, Clone, Copy)]
pub enum Dialect {
    /// SQLite
    Sqlite,
    /// PostgreSQL
    Postgres,
    /// MySQL/MariaDB
    Mysql,
}

impl Introspector {
    /// Create a new introspector for the given dialect.
    pub fn new(dialect: Dialect) -> Self {
        Self { dialect }
    }

    /// List all table names in the database.
    pub async fn table_names<C: Connection>(
        &self,
        cx: &Cx,
        conn: &C,
    ) -> Outcome<Vec<String>, Error> {
        let sql = match self.dialect {
            Dialect::Sqlite => {
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
            }
            Dialect::Postgres => {
                "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
            }
            Dialect::Mysql => "SHOW TABLES",
        };

        let rows = match conn.query(cx, sql, &[]).await {
            Outcome::Ok(rows) => rows,
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        };

        let names: Vec<String> = rows
            .iter()
            .filter_map(|row| row.get(0).and_then(|v| v.as_str().map(String::from)))
            .collect();

        Outcome::Ok(names)
    }

    /// Get detailed information about a table.
    pub async fn table_info<C: Connection>(
        &self,
        cx: &Cx,
        conn: &C,
        table_name: &str,
    ) -> Outcome<TableInfo, Error> {
        let columns = match self.columns(cx, conn, table_name).await {
            Outcome::Ok(cols) => cols,
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        };

        let primary_key: Vec<String> = columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.clone())
            .collect();

        // TODO: Implement foreign key and index introspection
        let foreign_keys = Vec::new();
        let indexes = Vec::new();

        Outcome::Ok(TableInfo {
            name: table_name.to_string(),
            columns,
            primary_key,
            foreign_keys,
            indexes,
        })
    }

    /// Get column information for a table.
    async fn columns<C: Connection>(
        &self,
        cx: &Cx,
        conn: &C,
        table_name: &str,
    ) -> Outcome<Vec<ColumnInfo>, Error> {
        match self.dialect {
            Dialect::Sqlite => self.sqlite_columns(cx, conn, table_name).await,
            Dialect::Postgres => self.postgres_columns(cx, conn, table_name).await,
            Dialect::Mysql => self.mysql_columns(cx, conn, table_name).await,
        }
    }

    async fn sqlite_columns<C: Connection>(
        &self,
        cx: &Cx,
        conn: &C,
        table_name: &str,
    ) -> Outcome<Vec<ColumnInfo>, Error> {
        let sql = format!("PRAGMA table_info({})", table_name);
        let rows = match conn.query(cx, &sql, &[]).await {
            Outcome::Ok(rows) => rows,
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        };

        let columns: Vec<ColumnInfo> = rows
            .iter()
            .filter_map(|row| {
                let name = row.get_named::<String>("name").ok()?;
                let sql_type = row.get_named::<String>("type").ok()?;
                let notnull = row.get_named::<i64>("notnull").ok().unwrap_or(0);
                let dflt_value = row.get_named::<String>("dflt_value").ok();
                let pk = row.get_named::<i64>("pk").ok().unwrap_or(0);

                Some(ColumnInfo {
                    name,
                    sql_type,
                    nullable: notnull == 0,
                    default: dflt_value,
                    primary_key: pk > 0,
                    auto_increment: false, // SQLite doesn't report this via PRAGMA
                })
            })
            .collect();

        Outcome::Ok(columns)
    }

    async fn postgres_columns<C: Connection>(
        &self,
        cx: &Cx,
        conn: &C,
        table_name: &str,
    ) -> Outcome<Vec<ColumnInfo>, Error> {
        let sql = "SELECT column_name, data_type, is_nullable, column_default
                   FROM information_schema.columns
                   WHERE table_name = $1
                   ORDER BY ordinal_position";

        let rows = match conn
            .query(
                cx,
                sql,
                &[sqlmodel_core::Value::Text(table_name.to_string())],
            )
            .await
        {
            Outcome::Ok(rows) => rows,
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        };

        let columns: Vec<ColumnInfo> = rows
            .iter()
            .filter_map(|row| {
                let name = row.get_named::<String>("column_name").ok()?;
                let sql_type = row.get_named::<String>("data_type").ok()?;
                let nullable_str = row.get_named::<String>("is_nullable").ok()?;
                let default = row.get_named::<String>("column_default").ok();

                Some(ColumnInfo {
                    name,
                    sql_type,
                    nullable: nullable_str == "YES",
                    default,
                    primary_key: false, // Need separate query
                    auto_increment: false,
                })
            })
            .collect();

        Outcome::Ok(columns)
    }

    async fn mysql_columns<C: Connection>(
        &self,
        cx: &Cx,
        conn: &C,
        table_name: &str,
    ) -> Outcome<Vec<ColumnInfo>, Error> {
        let sql = format!("DESCRIBE {}", table_name);
        let rows = match conn.query(cx, &sql, &[]).await {
            Outcome::Ok(rows) => rows,
            Outcome::Err(e) => return Outcome::Err(e),
            Outcome::Cancelled(r) => return Outcome::Cancelled(r),
            Outcome::Panicked(p) => return Outcome::Panicked(p),
        };

        let columns: Vec<ColumnInfo> = rows
            .iter()
            .filter_map(|row| {
                let name = row.get_named::<String>("Field").ok()?;
                let sql_type = row.get_named::<String>("Type").ok()?;
                let null = row.get_named::<String>("Null").ok()?;
                let key = row.get_named::<String>("Key").ok()?;
                let default = row.get_named::<String>("Default").ok();
                let extra = row.get_named::<String>("Extra").ok().unwrap_or_default();

                Some(ColumnInfo {
                    name,
                    sql_type,
                    nullable: null == "YES",
                    default,
                    primary_key: key == "PRI",
                    auto_increment: extra.contains("auto_increment"),
                })
            })
            .collect();

        Outcome::Ok(columns)
    }
}
