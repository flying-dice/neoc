from __future__ import annotations
from dataclasses import dataclass
from .neoc_lib import Result, Option, Ok, Err, Some, None_

from .autosql.migrate import migrate
from .autosql.add_note import add_note
from .autosql.list_notes import ListNotesRow
from .autosql.list_notes import list_notes
@dataclass(frozen=True)
class HttpResponse:
    status: float
    body: str

@dataclass(frozen=True)
class NoteLine:
    id: float
    body: str
    def display(self) -> str:
        return str(self.id) + ", " + str(self.body)

def init(db: E) -> Result[float, str]:
    return migrate(db)

def render(notes: list[ListNotesRow]) -> str:
    out = ""
    for n in notes:
        line = NoteLine(id=n.id, body=n.body)
        out = ((out + line.display()) + " | ")
    return out

def list(db: E) -> HttpResponse:
    match list_notes(db):
        case Ok(notes):
            _m1 = HttpResponse(status=200, body=render(notes))
        case Err(e):
            _m1 = HttpResponse(status=500, body=("error: " + e))
    return _m1

def create(db: E, body: str) -> HttpResponse:
    match add_note(db, body):
        case Ok(_):
            _m2 = HttpResponse(status=200, body="added")
        case Err(e):
            _m2 = HttpResponse(status=500, body=("error: " + e))
    return (HttpResponse(status=400, body="body must not be empty") if (body == "") else _m2)


__all__ = ["HttpResponse", "init", "list", "create"]
