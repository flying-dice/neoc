from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    err: Result[float, str] = Err("boom")
    println(err.unwrap_err())

