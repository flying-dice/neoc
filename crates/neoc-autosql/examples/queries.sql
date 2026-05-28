-- Example queries for `neoc autosql`. Each is preceded by a
-- `-- name: <Name> :one|:many|:exec` annotation (sqlc-style).

-- name: UserById :one
SELECT id, name, email, active FROM users WHERE id = ?;

-- name: ActiveUsers :many
SELECT id, name FROM users WHERE active = ?;

-- name: AllUsers :many
SELECT * FROM users;

-- name: InsertUser :exec
INSERT INTO users (id, name, email, active) VALUES (?, ?, ?, ?);

-- name: DeleteUser :exec
DELETE FROM users WHERE id = ?;
