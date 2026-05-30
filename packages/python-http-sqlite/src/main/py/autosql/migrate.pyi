from dataclasses import dataclass
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def migrate(db: E) -> Result[float, str]: ...
