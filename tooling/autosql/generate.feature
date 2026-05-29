Feature: Generating a neoc data-access layer

  autosql turns a SQL schema and a set of single-query files into a typed neoc
  data-access layer: one shared `ports.neoc` (the `SqlValue` enum, the
  `SqlExecutor` trait, and the bind/decode helpers), one `migrate.neoc` runner,
  and one self-contained `.neoc` file per query under `autosql/`. The emitted
  code is pure neoc — it couples to no runtime; the only seam to a database is a
  user-written `impl SqlExecutor`.

  Scenario: The shared ports and migrate files are always generated
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    When I generate for sqlite
    Then it generates a file "ports.neoc"
    And it generates a file "migrate.neoc"

  Scenario: Ports holds the SqlValue/SqlExecutor contract and the helpers
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    When I generate for sqlite
    Then the generated "ports.neoc" contains "pub enum SqlValue"
    And the generated "ports.neoc" contains "pub trait SqlExecutor"
    And the generated "ports.neoc" contains "pub fn decode_number"
    And the generated "ports.neoc" contains "pub fn decode_opt_string"
    And the generated "ports.neoc" contains "pub fn bind_opt_text"

  Scenario: A SELECT query becomes a typed row struct and query function
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      """
    And the query "user_by_id":
      """
      SELECT id, name, email FROM users WHERE id = ?;
      """
    When I generate for sqlite
    Then it generates a file "user_by_id.neoc"
    And the generated "user_by_id.neoc" contains:
      """
      pub struct UserByIdRow {
          id: number,
          name: string,
          email: Option<string>,
      }
      """
    And the generated "user_by_id.neoc" contains:
      """
      pub fn user_by_id<E: SqlExecutor>(db: E, id: number) -> Result<[UserByIdRow], string> {
      """

  Scenario: A nullable column becomes an Option field
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      """
    And the query "user_by_id":
      """
      SELECT id, name, email FROM users WHERE id = ?;
      """
    When I generate for sqlite
    Then the generated "user_by_id.neoc" contains "name: string,"
    And the generated "user_by_id.neoc" contains "email: Option<string>,"

  Scenario: A LEFT JOIN makes the joined side nullable in the row struct
    Given the schema:
      """
      CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      CREATE TABLE books (id INTEGER PRIMARY KEY, author_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "books_with_author":
      """
      SELECT books.title AS title, authors.name AS author
      FROM books LEFT JOIN authors ON authors.id = books.author_id;
      """
    When I generate for sqlite
    Then the generated "books_with_author.neoc" contains "title: string,"
    And the generated "books_with_author.neoc" contains "author: Option<string>,"

  Scenario: A non-SELECT statement returns the rows-affected count
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    And the query "remove_user":
      """
      DELETE FROM users WHERE id = ?;
      """
    When I generate for sqlite
    Then the generated "remove_user.neoc" contains:
      """
      pub fn remove_user<E: SqlExecutor>(db: E, id: number) -> Result<number, string> {
      """

  Scenario: INSERT ... RETURNING keeps the rows-affected return but lists its columns
    Given the schema:
      """
      CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL, bio TEXT);
      """
    And the query "create_author":
      """
      INSERT INTO authors (name, bio) VALUES (?, ?) RETURNING id, name;
      """
    When I generate for sqlite
    Then the generated "create_author.neoc" contains:
      """
      pub fn create_author<E: SqlExecutor>(db: E, name: string, bio: Option<string>) -> Result<number, string>
      """

  Scenario: The query SQL is embedded verbatim in the call
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    And the query "user_by_id":
      """
      SELECT id, name FROM users WHERE id = $1;
      """
    When I generate for postgres
    Then the generated "user_by_id.neoc" contains "WHERE id = $1"

  Scenario: A keyword column name is sanitised to a valid neoc identifier
    Given the schema:
      """
      CREATE TABLE items (id INTEGER PRIMARY KEY, "type" TEXT NOT NULL);
      """
    And the query "all_items":
      """
      SELECT id, "type" FROM items;
      """
    When I generate for sqlite
    Then the generated "all_items.neoc" contains "type_: string,"

  Scenario: Every generated file is stamped as generated, do not edit
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    When I generate for sqlite
    Then the generated "ports.neoc" contains "// @generated by neoc autosql — do not edit."
    And the generated "migrate.neoc" contains "// @generated by neoc autosql — do not edit."

  Scenario: The migrate runner imports the executor and runs the DDL
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    When I generate for sqlite
    Then the generated "migrate.neoc" contains "use autosql::ports::SqlExecutor;"
    And the generated "migrate.neoc" contains "pub fn migrate<E: SqlExecutor>(db: E) -> Result<number, string>"
    And the generated "migrate.neoc" contains "CREATE TABLE users"
