from __future__ import annotations
from ..neoc_lib import Result, Option, Ok, Err, Some, None_

from .ports import SqlValue
from .ports import Int, Real, Text, Bool, Null
def add_note(db: E, body: str) -> Result[float, str]:
    params: list[SqlValue] = [Text(body)]
    return db.execute("INSERT INTO notes (body) VALUES (?)", params)


__all__ = ["add_note"]
