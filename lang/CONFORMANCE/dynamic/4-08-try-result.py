from __future__ import annotations
from .neoc_lib import Result, Option, Ok, Err, Some, None_

def parse(ok: bool) -> Result[float, str]:
    return (Ok(10) if ok else Err("bad"))

def doubled(ok: bool) -> Result[float, str]:
    _try0 = parse(ok)
    if isinstance(_try0, (Err, None_)):
        return _try0
    n = _try0._0
    return Ok((n + n))

def show(r: Result[float, str]) -> str:
    match r:
        case Ok(n):
            _m1 = "ok"
        case Err(e):
            _m1 = e
    return _m1

def main() -> None:
    println(show(doubled(True)))
    println(show(doubled(False)))

