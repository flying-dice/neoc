# The application host, in Python — the entry point and the driver.
#
# neoc owns the contracts (the controllers + the `SqlExecutor` port + the
# `Result` / `Option` / `SqlValue` shapes), transpiled beside the source in
# `../neoc/*` with `.pyi` stubs. This host does the three things the model asks:
#   1. implements the port neoc declares — the `SqlExecutor` adapter over the
#      stdlib `sqlite3` module (the only code that touches a database);
#   2. owns HTTP — `http.server` and the request routing;
#   3. maps each controller's `HttpResponse` (status + body, both decided in
#      neoc) onto an HTTP response. The controllers own *what* the response is;
#      the host owns only *how* to express it.
#
# Note: the host never touches neoc's runtime encoding. It builds/reads
# `SqlValue` through the emitted `@dataclass` variants (`Int` / `Real` / `Text`
# / `Bool` / `Null`) and a structural `match`; the tag fields never appear here.
# The `Result` handling lives in the controllers, so the host only reads
# `HttpResponse.{status, body}`.
import sqlite3
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import unquote

# The compiled neoc lands beside the source in ../neoc. Its modules use relative
# imports (`from .autosql.ports import …`, `from ..neoc_lib import …`), so the
# output must be an importable package. `neoc build` emits only `.py`/`.pyi`, so
# mark the directories as packages here (idempotent), then put `src/main` on the
# path and import the `neoc` package so the relative imports resolve.
_ROOT = Path(__file__).resolve().parents[3]
for _pkg in (_ROOT / "out", _ROOT / "out" / "autosql"):
    (_pkg / "__init__.py").touch(exist_ok=True)
sys.path.insert(0, str(_ROOT))

from out.neoc_lib import Ok, Err  # noqa: E402
from out.autosql.ports import Int, Real, Text, Bool, Null  # noqa: E402
from out.main import init, list as list_notes, create  # noqa: E402

PORT = 38573


# --- the adapter: implement the `SqlExecutor` port over sqlite3 ---


def to_bound(v):
    """A `SqlValue` -> the Python value sqlite3 binds. Structural `match` on the
    emitted variant dataclasses; no tag field is ever read."""
    match v:
        case Int(n) | Real(n):
            return n
        case Text(s):
            return s
        case Bool(b):
            return 1 if b else 0
        case Null():
            return None
    raise TypeError(f"unbindable SqlValue: {v!r}")


def to_cell(value):
    """A raw sqlite3 cell -> `SqlValue`, via the emitted variant constructors.
    sqlite3 returns int/float for numbers, str for text, None for NULL; tag each
    back into the dataclass variant the neoc decoders expect."""
    if isinstance(value, bool):
        return Bool(value)
    if isinstance(value, (int, float)):
        return Real(float(value))
    if isinstance(value, str):
        return Text(value)
    return Null()


class SqliteExecutor:
    """The `SqlExecutor` port, implemented over an in-memory sqlite3 connection.
    `execute` returns `Ok(changes)` / `Err(msg)`; `query` returns
    `Ok([[SqlValue]])` / `Err(msg)` — the neoc `Result` the controllers `?` on."""

    def __init__(self, connection):
        self._db = connection

    def execute(self, sql, params):
        try:
            cur = self._db.execute(sql, [to_bound(p) for p in params])
            self._db.commit()
            return Ok(float(cur.rowcount))
        except Exception as e:  # noqa: BLE001 — any DB error becomes a neoc Err
            return Err(str(e))

    def query(self, sql, params):
        try:
            cur = self._db.execute(sql, [to_bound(p) for p in params])
            rows = [[to_cell(cell) for cell in row] for row in cur.fetchall()]
            return Ok(rows)
        except Exception as e:  # noqa: BLE001 — any DB error becomes a neoc Err
            return Err(str(e))


# --- the host owns HTTP and dispatches routes to the neoc controllers ---


def make_handler(db):
    class Handler(BaseHTTPRequestHandler):
        def _send(self, response):
            # Map the controller's `HttpResponse` (status + body, decided in
            # neoc) onto the HTTP response.
            body = response.body.encode()
            self.send_response(int(response.status))
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            # /add/<text> -> insert a note (the param arrives URL-encoded);
            # anything else -> list every note. The controller decides the
            # status + body; the host just maps it.
            prefix = "/add/"
            if self.path.startswith(prefix):
                text = unquote(self.path[len(prefix):])
                self._send(create(db, text))
            else:
                self._send(list_notes(db))

        def log_message(self, *_args):
            pass  # quiet; the demo prints its own startup line

    return Handler


def main():
    # --- the adapter over an in-memory database ---
    db = SqliteExecutor(sqlite3.connect(":memory:", check_same_thread=False))

    # --- startup: schema + seed, by calling the neoc controllers ---
    init(db)
    create(db, "hello")
    create(db, "world")

    server = HTTPServer(("127.0.0.1", PORT), make_handler(db))
    print(f"listening on http://localhost:{PORT}")
    server.serve_forever()


if __name__ == "__main__":
    main()
