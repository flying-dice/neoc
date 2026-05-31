from dataclasses import dataclass
from .neoc_lib import Result, Option, Ok, Err, Some, None_
from .autosql.ports import SqlExecutor
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

def init(db: SqlExecutor) -> Result[float, str]: ...
def list_(db: SqlExecutor) -> HttpResponse: ...
def create(db: SqlExecutor, body: str) -> HttpResponse: ...
