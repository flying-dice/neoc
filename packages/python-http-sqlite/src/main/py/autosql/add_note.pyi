from dataclasses import dataclass
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def add_note(db: E, body: str) -> Result[float, str]: ...
