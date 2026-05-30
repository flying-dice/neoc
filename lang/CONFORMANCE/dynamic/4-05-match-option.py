from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def describe(o: Option[float]) -> str:
    match o:
        case Some(n):
            _m1 = "some"
        case None_():
            _m1 = "none"
    return _m1

def main() -> None:
    print(describe(Some(3)))
    print(describe(None_()))

