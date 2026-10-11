Feature: Inferring queries beyond a single flat SELECT

  autosql types CTEs, derived tables, subqueries, set operations, and window
  functions, following the SQL standard. A CTE (`WITH name AS (…)`) and a
  derived table (`FROM (SELECT …) AS name`) expose their query's result columns
  — with that query's nullability — to the rest of the statement; an outer join
  to either widens its columns to optional. A subquery in an expression may
  refer to the enclosing query's columns (a correlated reference).

  Expression rules:

    - a scalar subquery `(SELECT …)` has its column's type and is always
      optional (no row yields NULL);
    - `EXISTS (…)` is a non-null `bool`;
    - `x IN (SELECT …)` is a `bool`, optional when `x` or the subquery column
      is optional (SQL three-valued logic).

  A set operation takes its field names from the left side; both sides must
  project the same number of columns of the same neoc type. A result column is
  optional for `UNION` if either side is, for `EXCEPT` if the left is, and for
  `INTERSECT` only if both are.

  A window function is typed by its kind: `row_number`/`rank`/`dense_rank`/
  `ntile`/`cume_dist`/`percent_rank` are non-null numbers; `lag`/`lead` are
  optional unless a non-null default covers the missing row; `first_value`/
  `last_value` follow their argument under an inline window with the default
  frame and are optional otherwise; `nth_value` is optional; an aggregate over
  a window types as the aggregate does. Placeholders bind in textual order,
  subqueries, CTEs, derived tables, and `JOIN … ON` included. `WITH RECURSIVE`,
  `LATERAL`, and a derived table without an alias are refused.

  Scenario: A CTE exposes its columns, with their nullability, to the query
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "users_via_cte":
      """
      WITH named AS (SELECT id, email FROM users)
      SELECT id, email FROM named WHERE id = ?;
      """
    When I generate for sqlite
    Then the generated "users_via_cte.neoc" contains:
      """
      pub struct UsersViaCteRow {
          id: number,
          email: Option<string>,
      }
      """
    And the generated "users_via_cte.neoc" contains "id: number) -> Result"

  Scenario: A CTE column-alias list renames the CTE's columns
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "renamed_cte":
      """
      WITH named(user_id, mail) AS (SELECT id, email FROM users)
      SELECT user_id, mail FROM named;
      """
    When I generate for sqlite
    Then the generated "renamed_cte.neoc" contains "user_id: number,"
    And the generated "renamed_cte.neoc" contains "mail: Option<string>,"

  Scenario: Placeholders bind in textual order across CTEs and the body
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "ordered":
      """
      WITH a AS (SELECT id FROM users WHERE name = ?)
      SELECT id FROM a WHERE id > ?;
      """
    When I generate for sqlite
    Then the generated "ordered.neoc" contains "db: E, name: string, id: number)"

  Scenario: An outer join to a CTE makes its columns optional
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "user_titles":
      """
      WITH t AS (SELECT user_id, title FROM posts)
      SELECT users.name, t.title FROM users LEFT JOIN t ON t.user_id = users.id;
      """
    When I generate for sqlite
    Then the generated "user_titles.neoc" contains "name: string,"
    And the generated "user_titles.neoc" contains "title: Option<string>,"

  Scenario: A derived table exposes its projection
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "post_total":
      """
      SELECT t.total FROM (SELECT count(*) AS total FROM posts WHERE user_id = ?) AS t;
      """
    When I generate for sqlite
    Then the generated "post_total.neoc" contains "total: number,"
    And the generated "post_total.neoc" contains "user_id: number)"

  Scenario: A scalar subquery is optional and may be correlated
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "newest_post":
      """
      SELECT users.name,
        (SELECT max(posts.id) FROM posts WHERE posts.user_id = users.id) AS newest
      FROM users;
      """
    When I generate for sqlite
    Then the generated "newest_post.neoc" contains "name: string,"
    And the generated "newest_post.neoc" contains "newest: Option<number>,"

  Scenario: EXISTS is a non-null bool and needs no alias on its select item
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "has_posts":
      """
      SELECT id, EXISTS (SELECT 1 FROM posts WHERE posts.user_id = users.id) AS active FROM users;
      """
    When I generate for sqlite
    Then the generated "has_posts.neoc" contains "active: bool,"

  Scenario: IN (SELECT …) binds the subquery's placeholders
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "users_who_posted":
      """
      SELECT name FROM users
      WHERE id IN (SELECT user_id FROM posts WHERE title = ?);
      """
    When I generate for sqlite
    Then the generated "users_who_posted.neoc" contains "title: string)"

  Scenario: A subquery in DELETE ... WHERE binds its placeholders
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "delete_posts_of":
      """
      DELETE FROM posts WHERE user_id IN (SELECT id FROM users WHERE name = ?);
      """
    When I generate for sqlite
    Then the generated "delete_posts_of.neoc" contains "name: string) -> Result<number, string>"

  Scenario: UNION takes field names from the left and is optional if either side is
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "contacts":
      """
      SELECT id, email AS contact FROM users UNION SELECT id, title FROM posts;
      """
    When I generate for sqlite
    Then the generated "contacts.neoc" contains "contact: Option<string>,"

  Scenario: INTERSECT is optional only when both sides are
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "shared":
      """
      SELECT email FROM users INTERSECT SELECT title FROM posts;
      """
    When I generate for sqlite
    Then the generated "shared.neoc" contains "email: string,"

  Scenario: EXCEPT keeps the left side's nullability
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "unique_titles":
      """
      SELECT title FROM posts EXCEPT SELECT email FROM users;
      """
    When I generate for sqlite
    Then the generated "unique_titles.neoc" contains "title: string,"

  Scenario: Set-operation placeholders bind left, right, then LIMIT
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "either":
      """
      SELECT id FROM users WHERE name = ?
      UNION ALL SELECT user_id FROM posts WHERE title = ?
      LIMIT ?;
      """
    When I generate for sqlite
    Then the generated "either.neoc" contains "db: E, name: string, title: string, limit: number)"

  Scenario: Ranking window functions are non-null numbers
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "post_places":
      """
      SELECT title, row_number() OVER (PARTITION BY user_id ORDER BY id) AS place FROM posts;
      """
    When I generate for sqlite
    Then the generated "post_places.neoc" contains "place: number,"

  Scenario: lag is optional unless a non-null default is given
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "post_history":
      """
      SELECT lag(title) OVER (ORDER BY id) AS previous,
        lag(title, 1, 'none') OVER (ORDER BY id) AS previous_or_none
      FROM posts;
      """
    When I generate for sqlite
    Then the generated "post_history.neoc" contains "previous: Option<string>,"
    And the generated "post_history.neoc" contains "previous_or_none: string,"

  Scenario: An aggregate over a window types as the aggregate
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "running":
      """
      SELECT count(*) OVER () AS everyone, sum(id) OVER (PARTITION BY user_id) AS subtotal FROM posts;
      """
    When I generate for sqlite
    Then the generated "running.neoc" contains "everyone: number,"
    And the generated "running.neoc" contains "subtotal: Option<number>,"

  Scenario: Subqueries and set operations also generate for postgres
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "pg_union":
      """
      SELECT id FROM users WHERE id > $1 UNION SELECT user_id FROM posts WHERE id < $2;
      """
    When I generate for postgres
    Then the generated "pg_union.neoc" contains "WHERE id > $1 UNION"

  # --- unhappy paths -------------------------------------------------------

  @chaos
  Scenario: A set operation whose sides differ in column count is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "bad_union":
      """
      SELECT id, name FROM users UNION SELECT id FROM posts;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "2 and 1 column(s)"

  @chaos
  Scenario: A set operation whose columns differ in type is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "mixed_union":
      """
      SELECT id FROM users UNION SELECT title FROM posts;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "column 1 is `number` on the left but `string` on the right"

  @chaos
  Scenario: A scalar subquery projecting two columns is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "wide_scalar":
      """
      SELECT (SELECT id, name FROM users) AS x FROM users;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "must project exactly one column"

  @chaos
  Scenario: An IN subquery projecting two columns is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "wide_in":
      """
      SELECT id FROM users WHERE id IN (SELECT id, user_id FROM posts);
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "must project exactly one column"

  @chaos
  Scenario: An ambiguous column inside a subquery is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "ambiguous_inner":
      """
      SELECT name FROM users
      WHERE id IN (SELECT id FROM users JOIN posts ON posts.user_id = users.id);
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "ambiguous"

  @chaos
  Scenario: An unknown column inside a subquery is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "unknown_inner":
      """
      SELECT id FROM users WHERE id IN (SELECT nope FROM posts);
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "column `nope` is not in any table in scope"

  @chaos
  Scenario: An unknown column in a CTE body is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "unknown_cte":
      """
      WITH t AS (SELECT nope FROM users) SELECT * FROM t;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "column `nope`"

  @chaos
  Scenario: A CTE alias list with the wrong arity is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "bad_cte_alias":
      """
      WITH t(a) AS (SELECT id, name FROM users) SELECT a FROM t;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "has 2 column(s) but its alias lists 1"

  @chaos
  Scenario: A recursive CTE is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "tree":
      """
      WITH RECURSIVE t AS (SELECT id FROM users UNION ALL SELECT id FROM t) SELECT id FROM t;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "WITH RECURSIVE is not supported"

  @chaos
  Scenario: A derived table without an alias is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "unnamed_derived":
      """
      SELECT id FROM (SELECT id FROM users);
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "needs an alias"

  @chaos
  Scenario: A LATERAL derived table is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "lateral_derived":
      """
      SELECT u.id FROM users AS u, LATERAL (SELECT id FROM posts) AS p;
      """
    When I generate for postgres
    Then generation is refused
    And the error mentions "LATERAL"

  @chaos
  Scenario: An unaliased expression in a derived table is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "unaliased_derived":
      """
      SELECT t.x FROM (SELECT id + 1 FROM users) AS t;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "needs an alias"

  @chaos
  Scenario: A window naming an unknown partition column is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "bad_window":
      """
      SELECT row_number() OVER (PARTITION BY nope) AS n FROM posts;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "column `nope`"

  @chaos
  Scenario: A placeholder in a projection subquery nobody can bind is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "unbound":
      """
      SELECT id + ? AS bumped FROM users;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "bind placeholder"

  Scenario: JOIN … ON and derived-table placeholders bind in textual order
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "interleaved":
      """
      SELECT u.name FROM users u JOIN posts p ON p.user_id = u.id AND p.id = ? JOIN (SELECT id FROM users WHERE name = ?) d ON d.id = u.id;
      """
    When I generate for sqlite
    Then the generated "interleaved.neoc" contains "db: E, id: number, name: string)"

  Scenario: first_value under an explicit frame is optional
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "framed":
      """
      SELECT first_value(title) OVER (ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 2 FOLLOWING) AS f, first_value(title) OVER (ORDER BY id) AS d FROM posts;
      """
    When I generate for sqlite
    Then the generated "framed.neoc" contains "f: Option<string>,"
    And the generated "framed.neoc" contains "d: string,"

  Scenario: EXISTS accepts SELECT NULL
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "exists_null":
      """
      SELECT name FROM users WHERE EXISTS (SELECT NULL FROM posts WHERE posts.user_id = users.id);
      """
    When I generate for sqlite
    Then the generated "exists_null.neoc" contains "name: string,"

  Scenario: An INTEGER and a REAL set-operation column bind as Real
    Given the schema:
      """
      CREATE TABLE a (n INTEGER NOT NULL);
      CREATE TABLE b (r REAL NOT NULL);
      """
    And the query "mixed_numbers":
      """
      WITH v AS (SELECT n FROM a UNION SELECT r FROM b) SELECT n FROM v WHERE n = ?;
      """
    When I generate for sqlite
    Then the generated "mixed_numbers.neoc" contains "SqlValue::Real(n)"

  @chaos
  Scenario: A de-duplicated name a derived table never exposes is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "phantom_column":
      """
      SELECT t.id_2 FROM (SELECT u.id, p.id FROM users u JOIN posts p ON p.user_id = u.id) AS t;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "column `id_2` not found"

  @chaos
  Scenario: A name a derived table projects twice is ambiguous
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "twice_projected":
      """
      SELECT t.id FROM (SELECT u.id, p.id FROM users u JOIN posts p ON p.user_id = u.id) AS t;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "ambiguous"

  @chaos
  Scenario: An unknown column inside a window expression is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "bad_window_expr":
      """
      SELECT row_number() OVER (PARTITION BY lower(nope)) AS n FROM posts;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "column `nope`"

  @chaos
  Scenario: An undefined named window is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "undefined_window":
      """
      SELECT rank() OVER undefined_w AS r FROM posts;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "no window `undefined_w`"

  Scenario: A NULL operand makes the expression optional
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "null_operand":
      """
      SELECT name || NULL AS s, id + NULL AS a FROM users;
      """
    When I generate for sqlite
    Then the generated "null_operand.neoc" contains "s: Option<string>,"
    And the generated "null_operand.neoc" contains "a: Option<number>,"

  @chaos
  Scenario: A set operation inside EXISTS still needs matching types
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "exists_mismatch":
      """
      SELECT name FROM users WHERE EXISTS (SELECT id FROM users UNION SELECT title FROM posts);
      """
    When I generate for postgres
    Then generation is refused
    And the error mentions "`number` on the left but `string` on the right"

  @chaos
  Scenario: NULL set NULL inside EXISTS resolves to string
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "exists_null_null":
      """
      SELECT name FROM users WHERE EXISTS (SELECT NULL FROM posts INTERSECT SELECT NULL FROM users EXCEPT SELECT id FROM users);
      """
    When I generate for postgres
    Then generation is refused
    And the error mentions "`string` on the left but `number` on the right"
