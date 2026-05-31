from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    s = Some(1)
    n: Option[float] = None_()
    println(s.is_some())
    println(s.is_none())
    println(n.is_some())
    println(n.is_none())

