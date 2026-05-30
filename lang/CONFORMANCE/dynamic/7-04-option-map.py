from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    s = Some(20)
    n: Option[float] = None_()
    println(s.map((lambda x: (x + 1))).unwrap_or(0))
    println(n.map((lambda x: (x + 1))).unwrap_or(0))

