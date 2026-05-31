from dataclasses import dataclass
from ..neoc_lib import Result, Option, Ok, Err, Some, None_
from .ports import SqlExecutor
from .ports import SqlValue

def add_note(db: SqlExecutor, body: str) -> Result[float, str]: ...
