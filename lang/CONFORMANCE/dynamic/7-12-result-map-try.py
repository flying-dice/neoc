from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def step(r: Result[float, str]) -> Result[float, str]:
    _try0 = r.map((lambda x: (x + 10)))
    if isinstance(_try0, (Err, None_)):
        return _try0
    v = _try0._0
    return Ok(v)

def read(r: Result[float, str]) -> float:
    match r:
        case Ok(v):
            _m1 = v
        case Err(_):
            _m1 = 0
    return _m1

def main() -> None:
    println(read(step(Ok(5))))
    println(read(step(Err("nope"))))

