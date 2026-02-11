use asupersync::runtime::RuntimeBuilder;
use asupersync::{Cx, Outcome};

use sqlmodel::SchemaBuilder;
use sqlmodel::prelude::*;
use sqlmodel_sqlite::SqliteConnection;

fn unwrap_outcome<T>(outcome: Outcome<T, Error>) -> T {
    match outcome {
        Outcome::Ok(v) => v,
        Outcome::Err(e) => panic!("unexpected error: {e}"),
        Outcome::Cancelled(r) => panic!("cancelled: {r:?}"),
        Outcome::Panicked(p) => panic!("panicked: {p:?}"),
    }
}

#[derive(sqlmodel::Model, Debug, Clone, PartialEq)]
#[sqlmodel(table)]
struct User {
    #[sqlmodel(primary_key)]
    id: i64,
    name: String,
}

#[test]
fn sqlite_select_one_enforces_exactly_one_row() {
    let rt = RuntimeBuilder::current_thread()
        .build()
        .expect("create asupersync runtime");
    let cx = Cx::for_testing();

    rt.block_on(async {
        let conn = SqliteConnection::open_memory().expect("open sqlite memory db");

        let stmts = SchemaBuilder::new().create_table::<User>().build();
        for stmt in stmts {
            unwrap_outcome(conn.execute(&cx, &stmt, &[]).await);
        }

        unwrap_outcome(
            conn.execute(
                &cx,
                "INSERT INTO users (id, name) VALUES (?1, ?2)",
                &[Value::BigInt(1), Value::Text("Alice".to_string())],
            )
            .await,
        );
        unwrap_outcome(
            conn.execute(
                &cx,
                "INSERT INTO users (id, name) VALUES (?1, ?2)",
                &[Value::BigInt(2), Value::Text("Bob".to_string())],
            )
            .await,
        );

        let alice = unwrap_outcome(
            select!(User)
                .filter(Expr::col("id").eq(1_i64))
                .one(&cx, &conn)
                .await,
        );
        assert_eq!(
            alice,
            User {
                id: 1,
                name: "Alice".to_string()
            }
        );

        let none = select!(User)
            .filter(Expr::col("id").eq(999_i64))
            .one(&cx, &conn)
            .await;
        match none {
            Outcome::Err(Error::Custom(msg)) => assert!(msg.contains("found none")),
            other => panic!("expected custom none-row error, got {other:?}"),
        }

        let many = select!(User).one(&cx, &conn).await;
        match many {
            Outcome::Err(Error::Custom(msg)) => {
                assert!(
                    msg.contains("Expected one row, found 2"),
                    "unexpected message: {msg}"
                );
            }
            other => panic!("expected custom multi-row error, got {other:?}"),
        }
    });
}
