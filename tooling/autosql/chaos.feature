Feature: Refusing malformed input

  Generation fails loudly rather than emitting broken neoc. A schema or query
  that does not parse as SQL, a query that names a table the schema never
  declares, a query file that holds more than one statement, an ambiguous column
  across a join, and an unrecognised dialect are all refused — each with a
  message that speaks SQL and neoc, never the host language.

  @chaos
  Scenario: A schema that is not valid SQL is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "SQL parse error"

  @chaos
  Scenario: A query against a table the schema never declares is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    And the query "all_widgets":
      """
      SELECT id FROM widgets;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "unknown table `widgets`"

  @chaos
  Scenario: A query against an empty schema has no table to resolve against
    Given the schema:
      """
      -- no tables here, just a comment
      """
    And the query "everything":
      """
      SELECT id FROM things;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "unknown table `things`"

  @chaos
  Scenario: A query file holding two statements is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    And the query "two_at_once":
      """
      SELECT id FROM users; SELECT name FROM users;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "found 2 statements"

  @chaos
  Scenario: An ambiguous column across a join is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
      """
    And the query "ambiguous":
      """
      SELECT id FROM users JOIN posts ON posts.user_id = users.id;
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "ambiguous"

  @chaos
  Scenario: An unrecognised dialect name is refused before generation
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    When I generate for the dialect "oracle"
    Then generation is refused
    And the error mentions "unknown dialect `oracle`"

  @chaos
  Scenario: An INSERT without an explicit column list is refused
    Given the schema:
      """
      CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
      """
    And the query "blind_insert":
      """
      INSERT INTO users VALUES (1, 'ada');
      """
    When I generate for sqlite
    Then generation is refused
    And the error mentions "INSERT must list its columns explicitly"
