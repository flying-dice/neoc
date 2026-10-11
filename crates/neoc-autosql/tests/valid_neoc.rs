//! autosql's contract is **valid `.neoc` out**. These tests check the generated
//! data-access layer is well-formed neoc — it parses, resolves its imports, and
//! type-checks with no diagnostics — across the full feature surface (joins,
//! aggregates, expressions, RETURNING, multiple dialects). How that neoc lowers
//! to a target is the backends' concern, not autosql's; nothing here claims
//! anything about a target.

use std::fs;
use std::path::{Path, PathBuf};

use neoc_autosql::{Dialect, GeneratedFile, Migration, Query, generate_project};
use neoc_compiler::{Target, transpile_project};

/// A unique temp directory for one test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("neoc-autosql-{tag}-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Writes the generated files into a throwaway project, drops in a `main.neoc`
/// that `use`s each query function (so the imports are actually resolved), and
/// asserts the whole project parses, resolves, and type-checks clean.
fn assert_project_well_formed(label: &str, files: &[GeneratedFile], query_names: &[&str]) {
    let tmp = TempDir::new(label);
    let root = tmp.path();
    fs::write(
        root.join("neoc.toml"),
        "[package]\nname = \"t\"\ntarget = \"lua\"\n",
    )
    .unwrap();
    let src = root.join("src/main/neoc");
    fs::create_dir_all(&src).unwrap();
    for file in files {
        let dest = src.join(&file.relative_path);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, &file.contents).unwrap();
    }

    // A main that imports migrate and every query fn, forcing real resolution.
    let mut main = String::from("use autosql::migrate::migrate;\n");
    for name in query_names {
        let _ =
            std::fmt::Write::write_fmt(&mut main, format_args!("use autosql::{name}::{name};\n"));
    }
    fs::write(src.join("main.neoc"), main).unwrap();

    let out = transpile_project(&src.join("main.neoc"), Target::default())
        .expect("reading the temp project");
    assert!(
        out.diagnostics.is_empty(),
        "{label}: generated project is not well-formed:\n{:#?}",
        out.diagnostics
    );
}

fn library() -> Vec<Migration> {
    vec![Migration {
        order: 1,
        name: "init".into(),
        sql: "\
CREATE TABLE authors (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    bio  TEXT
);
CREATE TABLE books (
    id        INTEGER PRIMARY KEY,
    author_id INTEGER NOT NULL,
    title     TEXT NOT NULL,
    rating    REAL,
    published BOOLEAN NOT NULL
);"
        .into(),
    }]
}

fn q(name: &str, sql: &str) -> Query {
    Query {
        name: name.into(),
        sql: sql.into(),
    }
}

/// Finds a generated file by its file name (the last path segment).
fn find<'a>(files: &'a [GeneratedFile], file_name: &str) -> &'a GeneratedFile {
    files
        .iter()
        .find(|f| f.relative_path.ends_with(file_name))
        .unwrap_or_else(|| panic!("no generated file `{file_name}`"))
}

#[test]
fn full_feature_surface_is_well_formed() {
    let queries = vec![
        q(
            "list_books_by_author",
            "SELECT books.id, books.title, books.rating FROM books WHERE books.author_id = ?;",
        ),
        q(
            "books_with_author",
            "SELECT books.title, authors.name AS author \
             FROM books LEFT JOIN authors ON authors.id = books.author_id;",
        ),
        q(
            "author_stats",
            "SELECT count(*) AS books, avg(rating) AS avg_rating, max(title) AS latest \
             FROM books WHERE author_id = ?;",
        ),
        q(
            "search_books",
            "SELECT id, title FROM books WHERE title LIKE ? AND rating BETWEEN ? AND ? LIMIT ?;",
        ),
        q(
            "book_flag",
            "SELECT id, CASE WHEN published THEN title ELSE 'draft' END AS label \
             FROM books WHERE id = ?;",
        ),
        q(
            "display_name",
            "SELECT id, coalesce(bio, name) AS display FROM authors;",
        ),
        q(
            "create_author",
            "INSERT INTO authors (name, bio) VALUES (?, ?) RETURNING id, name;",
        ),
        q("touch_rating", "UPDATE books SET rating = ? WHERE id = ?;"),
        q("remove_book", "DELETE FROM books WHERE id = ?;"),
    ];

    let files = generate_project(&library(), &queries, Dialect::Sqlite).unwrap();
    let names: Vec<&str> = queries.iter().map(|q| q.name.as_str()).collect();
    assert_project_well_formed("library", &files, &names);

    // A LEFT JOIN nulls the joined side; the row struct carries it.
    let join = find(&files, "books_with_author.neoc");
    assert!(join.contents.contains("pub fn books_with_author"));
    // RETURNING gives an INSERT a row struct, but the kind stays exec.
    let create = find(&files, "create_author.neoc");
    assert!(create.contents.contains(
        "pub fn create_author<E: SqlExecutor>(db: E, name: string, bio: Option<string>) \
         -> Result<number, string>"
    ));
}

#[test]
fn query_breadth_is_well_formed() {
    let queries = vec![
        q(
            "prolific_authors",
            "WITH counts AS (SELECT author_id, count(*) AS n FROM books GROUP BY author_id) \
             SELECT authors.name, counts.n FROM authors JOIN counts ON counts.author_id = authors.id \
             WHERE counts.n > ?;",
        ),
        q(
            "authors_without_books",
            "SELECT id, name FROM authors \
             WHERE NOT EXISTS (SELECT 1 FROM books WHERE books.author_id = authors.id);",
        ),
        q(
            "authors_of_rated",
            "SELECT name FROM authors \
             WHERE id IN (SELECT author_id FROM books WHERE rating > ?);",
        ),
        q(
            "best_rating",
            "SELECT name, (SELECT max(rating) FROM books WHERE books.author_id = authors.id) AS best \
             FROM authors;",
        ),
        q(
            "titles_and_names",
            "SELECT title AS label FROM books UNION SELECT name AS label FROM authors;",
        ),
        q(
            "ranked_books",
            "SELECT title, row_number() OVER (PARTITION BY author_id ORDER BY rating) AS place, \
             lag(title) OVER (ORDER BY id) AS previous FROM books;",
        ),
        q(
            "recent_titles",
            "SELECT t.title FROM (SELECT title FROM books WHERE id > ?) AS t;",
        ),
    ];
    let files = generate_project(&library(), &queries, Dialect::Sqlite).unwrap();
    let names: Vec<&str> = queries.iter().map(|q| q.name.as_str()).collect();
    assert_project_well_formed("breadth", &files, &names);
}

#[test]
fn left_join_nullability_shows_in_the_row_struct() {
    let queries = vec![q(
        "books_with_author",
        "SELECT books.title AS title, authors.name AS author \
         FROM books LEFT JOIN authors ON authors.id = books.author_id;",
    )];
    let files = generate_project(&library(), &queries, Dialect::Sqlite).unwrap();
    assert_project_well_formed("join", &files, &["books_with_author"]);
    let file = find(&files, "books_with_author.neoc");
    assert!(
        file.contents.contains("title: string,"),
        "left side stays non-null:\n{}",
        file.contents
    );
    assert!(
        file.contents.contains("author: Option<string>,"),
        "right side is nullable:\n{}",
        file.contents
    );
}

#[test]
fn postgres_dollar_placeholders() {
    let migrations = vec![Migration {
        order: 1,
        name: "init".into(),
        sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);".into(),
    }];
    let queries = vec![
        q("user_by_id", "SELECT id, name FROM users WHERE id = $1;"),
        q("rename", "UPDATE users SET name = $1 WHERE id = $2;"),
    ];
    let files = generate_project(&migrations, &queries, Dialect::Postgres).unwrap();
    assert_project_well_formed("postgres", &files, &["user_by_id", "rename"]);
    let file = find(&files, "user_by_id.neoc");
    assert!(
        file.contents.contains("WHERE id = $1"),
        "placeholders emitted verbatim"
    );
}

#[test]
fn mysql_question_placeholders() {
    let migrations = vec![Migration {
        order: 1,
        name: "init".into(),
        sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);".into(),
    }];
    let queries = vec![q("user_by_id", "SELECT id, name FROM users WHERE id = ?;")];
    let files = generate_project(&migrations, &queries, Dialect::Mysql).unwrap();
    assert_project_well_formed("mysql", &files, &["user_by_id"]);
}

#[test]
fn keyword_columns_are_sanitised() {
    // `type` is a neoc keyword; quoted in SQL so it parses as a column name.
    let migrations = vec![Migration {
        order: 1,
        name: "init".into(),
        sql: "CREATE TABLE items (id INTEGER PRIMARY KEY, \"type\" TEXT NOT NULL);".into(),
    }];
    let queries = vec![q("all_items", "SELECT id, \"type\" FROM items;")];
    let files = generate_project(&migrations, &queries, Dialect::Sqlite).unwrap();
    assert_project_well_formed("keyword", &files, &["all_items"]);
    let file = find(&files, "all_items.neoc");
    assert!(
        file.contents.contains("type_: string,"),
        "keyword field is sanitised:\n{}",
        file.contents
    );
}

#[test]
fn migrate_runs_every_ddl_statement_in_order() {
    let migrations = vec![
        Migration {
            order: 2,
            name: "add-index".into(),
            sql: "CREATE INDEX idx_name ON users (name);".into(),
        },
        Migration {
            order: 1,
            name: "init".into(),
            sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);".into(),
        },
    ];
    let files = generate_project(&migrations, &[], Dialect::Sqlite).unwrap();
    let migrate = find(&files, "migrate.neoc");
    let create_at = migrate
        .contents
        .find("CREATE TABLE users")
        .expect("init runs");
    let index_at = migrate.contents.find("CREATE INDEX").expect("index runs");
    assert!(
        create_at < index_at,
        "migrations run in ascending order regardless of input order:\n{}",
        migrate.contents
    );
    assert!(
        migrate
            .contents
            .contains("use autosql::ports::SqlExecutor;")
    );
    assert_project_well_formed("migrate", &files, &[]);
}

/// Locks the exact output for a small, stable case — catches unintended drift in
/// the emitted ports and a query file.
#[test]
fn golden_small_query_file() {
    const GOLDEN: &str = r#"// @generated by neoc autosql — do not edit.
use autosql::ports::SqlExecutor;
use autosql::ports::SqlValue;
use autosql::ports::decode_number;
use autosql::ports::decode_opt_string;
use autosql::ports::decode_string;

pub struct UserByIdRow {
    id: number,
    name: string,
    email: Option<string>,
}

pub fn user_by_id<E: SqlExecutor>(db: E, id: number) -> Result<[UserByIdRow], string> {
    let params: [SqlValue] = [SqlValue::Int(id)];
    let rows = db.query("SELECT id, name, email FROM users WHERE id = ?", params)?;
    let out: [UserByIdRow] = [];
    let next = 0;
    for row in rows {
        let decoded = decode_user_by_id(row)?;
        out[next] = decoded;
        next = next + 1;
    }
    Ok(out)
}

fn decode_user_by_id(cols: [SqlValue]) -> Result<UserByIdRow, string> {
    let id = decode_number(cols[0], "user_by_id: column 0 (id)")?;
    let name = decode_string(cols[1], "user_by_id: column 1 (name)")?;
    let email = decode_opt_string(cols[2], "user_by_id: column 2 (email)")?;
    Ok(UserByIdRow { id, name, email })
}"#;

    let migrations = vec![Migration {
        order: 1,
        name: "init".into(),
        sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);".into(),
    }];
    let queries = vec![q(
        "user_by_id",
        "SELECT id, name, email FROM users WHERE id = ?;",
    )];
    let files = generate_project(&migrations, &queries, Dialect::Sqlite).unwrap();
    let file = find(&files, "user_by_id.neoc");
    assert_eq!(file.contents.trim_end(), GOLDEN.trim_end());
}

#[test]
fn ports_holds_the_contract_and_helpers() {
    let files = generate_project(&library(), &[], Dialect::Sqlite).unwrap();
    let ports = find(&files, "ports.neoc");
    assert!(ports.contents.contains("pub enum SqlValue"));
    assert!(ports.contents.contains("pub trait SqlExecutor"));
    assert!(ports.contents.contains("pub fn decode_number"));
    assert!(ports.contents.contains("pub fn decode_opt_string"));
    assert!(ports.contents.contains("pub fn bind_opt_text"));
    // The contract lives in ports — no query-specific structs leak in.
    assert!(!ports.contents.contains("Row {"));
}
