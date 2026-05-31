from dataclasses import dataclass
from ..neoc_lib import Result, Option, Ok, Err, Some, None_
from .ports import SqlExecutor

def migrate(db: SqlExecutor) -> Result[float, str]: ...
