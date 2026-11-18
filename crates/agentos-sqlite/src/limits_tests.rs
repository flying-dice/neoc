//! Value, row and byte limits, and how a blob leaves the database.

use super::*;
use agentos_application::PortError;

fn db() -> AppDb {
    AppDb::open(None, &[Migration::new(1, "init", "CREATE TABLE notes(id INTEGER PRIMARY KEY, body BLOB);")])
        .unwrap()
}

#[tokio::test]
async fn a_blob_leaves_the_database_as_base64_text_not_a_byte_array() {
    let r = db()
        .query("SELECT x'' AS a, x'66' AS b, x'666f' AS c, x'666f6f' AS d, x'00ff10fb' AS e".into(), vec![])
        .await
        .unwrap();
    assert_eq!(r[0], json!({"a": "", "b": "Zg==", "c": "Zm8=", "d": "Zm9v", "e": "AP8Q+w=="}));
}

#[tokio::test]
async fn a_huge_blob_is_refused_by_sqlite_before_it_is_allocated() {
    let db = db();
    let err = db.query("SELECT zeroblob(500000000) AS b".into(), vec![]).await.unwrap_err();
    assert!(matches!(err, DbError::Sqlite(_)), "{err}");
    assert!(err.to_string().contains("too big"), "{err}");
    assert!(db.exec("INSERT INTO notes(body) SELECT zeroblob(500000000)".into(), vec![]).await.is_err());
    assert_eq!(db.query("SELECT 1 AS n".into(), vec![]).await.unwrap()[0]["n"], 1);
}

#[tokio::test]
async fn a_query_returning_too_many_rows_fails_with_a_clear_error() {
    let db = db();
    let rows = |n: usize| {
        format!("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < {n}) SELECT x FROM c")
    };
    assert_eq!(db.query(rows(MAX_QUERY_ROWS), vec![]).await.unwrap().len(), MAX_QUERY_ROWS);
    let err = db.query(rows(MAX_QUERY_ROWS + 1), vec![]).await.unwrap_err();
    assert!(matches!(err, DbError::TooManyRows), "{err}");
    assert!(matches!(PortError::from(err), PortError::Rejected(m) if m.contains("rows")));
}

#[tokio::test]
async fn a_result_over_the_byte_cap_fails_even_when_it_has_few_rows() {
    let db = db();
    // each value is under the value limit; together they are over the byte cap
    let sql = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 3) \
               SELECT zeroblob(6000000) AS b FROM c";
    let err = db.query(sql.into(), vec![]).await.unwrap_err();
    assert!(matches!(err, DbError::ResultTooLarge), "{err}");
    assert!(matches!(PortError::from(err), PortError::Rejected(m) if m.contains("larger than")));
    assert_eq!(db.query("SELECT 1 AS n".into(), vec![]).await.unwrap()[0]["n"], 1);
}
