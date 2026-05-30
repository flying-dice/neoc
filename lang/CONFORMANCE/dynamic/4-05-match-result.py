from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def describe(r: Result[float, str]) -> str:
    match r:
        case Ok(n):
            _m1 = "ok"
        case Err(e):
            _m1 = e
    return _m1

def main() -> None:
    print(describe(Ok(7)))
    print(describe(Err("boom")))

