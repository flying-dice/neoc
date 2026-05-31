from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def read(r: Result[float, str]) -> float:
    match r:
        case Ok(v):
            _m1 = v
        case Err(_):
            _m1 = 0
    return _m1

def main() -> None:
    ok: Result[float, str] = Ok(10)
    err: Result[float, str] = Err("e")
    println(read(ok.map((lambda x: (x + 5)))))
    println(read(err.map((lambda x: (x + 5)))))

