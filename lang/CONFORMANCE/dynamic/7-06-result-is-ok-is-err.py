from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    ok: Result[float, str] = Ok(1)
    err: Result[float, str] = Err("bad")
    println(ok.is_ok())
    println(ok.is_err())
    println(err.is_ok())
    println(err.is_err())

