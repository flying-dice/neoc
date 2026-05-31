from __future__ import annotations

def yes_no(b: bool) -> str:
    match b:
        case True:
            _m1 = "yes"
        case False:
            _m1 = "no"
    return _m1

def main() -> None:
    println(yes_no(True))
    println(yes_no(False))

