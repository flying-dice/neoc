from dataclasses import dataclass
from .neoc_lib import Result, Option, Ok, Err, Some, None_

@dataclass(frozen=True)
class ListNotesRow:
    id: float
    body: str

def list_notes(db: E) -> Result[list[ListNotesRow], str]: ...
