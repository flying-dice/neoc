from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def dig(o: Option[Result[float, str]]) -> float:
    match o:
        case Some(Ok(n)):
            _m1 = n
        case Some(Err(_)):
            _m1 = (-1)
        case None_():
            _m1 = 0
    return _m1

def main() -> None:
    println(dig(Some(Ok(42))))
    println(dig(Some(Err("boom"))))
    println(dig(None_()))

