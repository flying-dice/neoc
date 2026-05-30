from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    s = Some(5)
    n: Option[float] = None_()
    println(s.ok_or("x").is_ok())
    println(n.ok_or("x").is_err())

