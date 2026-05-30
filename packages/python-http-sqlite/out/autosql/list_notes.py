from __future__ import annotations
from dataclasses import dataclass
from ..neoc_lib import Result, Option, Ok, Err, Some, None_

from .ports import SqlValue
from .ports import Int, Real, Text, Bool, Null
from .ports import decode_number
from .ports import decode_string
@dataclass(frozen=True)
class ListNotesRow:
    id: float
    body: str

def list_notes(db: E) -> Result[list[ListNotesRow], str]:
    params: list[SqlValue] = []
    _try0 = db.query("SELECT id, body FROM notes ORDER BY id", params)
    if isinstance(_try0, (Err, None_)):
        return _try0
    rows = _try0._0
    out: list[ListNotesRow] = []
    next = 0
    for row in rows:
        _try1 = decode_list_notes(row)
        if isinstance(_try1, (Err, None_)):
            return _try1
        decoded = _try1._0
        _idx1 = next
        _val2 = decoded
        while len(out) <= _idx1:
            out.append(None)
        out[_idx1] = _val2
        next = (next + 1)
    return Ok(out)

def decode_list_notes(cols: list[SqlValue]) -> Result[ListNotesRow, str]:
    _try2 = decode_number(cols[0], "list_notes: column 0 (id)")
    if isinstance(_try2, (Err, None_)):
        return _try2
    id = _try2._0
    _try3 = decode_string(cols[1], "list_notes: column 1 (body)")
    if isinstance(_try3, (Err, None_)):
        return _try3
    body = _try3._0
    return Ok(ListNotesRow(id=id, body=body))


__all__ = ["ListNotesRow", "list_notes"]
