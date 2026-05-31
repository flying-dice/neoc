from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def read_err(r: Result[float, str]) -> str:
    match r:
        case Ok(_):
            _m1 = "ok"
        case Err(e):
            _m1 = e
    return _m1

def main() -> None:
    ok: Result[float, str] = Ok(1)
    err: Result[float, str] = Err("x")
    println(read_err(ok.map_err((lambda e: e))))
    println(read_err(err.map_err((lambda e: e))))

