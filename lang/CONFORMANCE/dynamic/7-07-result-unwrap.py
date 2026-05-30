from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    ok: Result[float, str] = Ok(33)
    println(ok.unwrap())

