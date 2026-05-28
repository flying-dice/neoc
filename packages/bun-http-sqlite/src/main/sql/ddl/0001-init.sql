-- Initial schema for the notes demo. `neoc autosql` builds the migrate runner
-- and typed query functions from src/main/sql.
CREATE TABLE notes (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    body TEXT NOT NULL
);
