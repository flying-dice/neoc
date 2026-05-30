from __future__ import annotations
from ..neoc_lib import Result, Option, Ok, Err, Some, None_

def migrate(db: E) -> Result[float, str]:
    applied = 0
    _try0 = db.execute("CREATE TABLE notes ( id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL )", [])
    if isinstance(_try0, (Err, None_)):
        return _try0
    _try0._0
    applied = (applied + 1)
    return Ok(applied)


__all__ = ["migrate"]
