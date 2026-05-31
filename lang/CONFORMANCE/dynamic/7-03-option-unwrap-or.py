from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    s = Some(7)
    n: Option[float] = None_()
    println(s.unwrap_or(0))
    println(n.unwrap_or(99))

