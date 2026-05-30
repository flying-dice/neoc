from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    s = Some(42)
    println(s.unwrap())

