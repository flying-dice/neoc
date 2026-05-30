from dataclasses import dataclass
from .neoc_lib import Result, Option, Ok, Err, Some, None_

@dataclass(frozen=True)
class HttpResponse:
    status: float
    body: str

@dataclass(frozen=True)
class NoteLine:
    id: float
    body: str

def init(db: E) -> Result[float, str]: ...
def list(db: E) -> HttpResponse: ...
def create(db: E, body: str) -> HttpResponse: ...
