-- Example schema for `neoc autosql`. Only CREATE TABLE statements are read.
CREATE TABLE users (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL,
    email  TEXT,            -- nullable → Option<string>
    active BOOLEAN NOT NULL
);
