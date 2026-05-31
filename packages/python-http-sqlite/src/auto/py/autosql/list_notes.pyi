from dataclasses import dataclass
from ..neoc_lib import Result, Option, Ok, Err, Some, None_
from .ports import SqlExecutor
from .ports import SqlValue
from .ports import decode_number
from .ports import decode_string

@dataclass(frozen=True)
class ListNotesRow:
    id: float
    body: str

def list_notes(db: SqlExecutor) -> Result[list[ListNotesRow], str]: ...
