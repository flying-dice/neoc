from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def main() -> None:
    s = Some(4)
    match s.map((lambda x: (x * 2))):
        case Some(v):
            _m1 = println(v)
        case None_():
            _m1 = println(0)
    return _m1

