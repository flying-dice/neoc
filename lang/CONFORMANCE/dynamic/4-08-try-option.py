from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def incr(o: Option[float]) -> Option[float]:
    _try0 = o
    if isinstance(_try0, (Err, None_)):
        return _try0
    n = _try0._0
    return Some((n + 1))

def unwrap_or(o: Option[float]) -> float:
    match o:
        case Some(n):
            _m1 = n
        case None_():
            _m1 = 0
    return _m1

def main() -> None:
    println(unwrap_or(incr(Some(41))))
    println(unwrap_or(incr(None_())))

