from __future__ import annotations

def classify(n: float) -> float:
    match n:
        case 0:
            _m1 = 100
        case 1:
            _m1 = 200
        case _:
            _m1 = 999
    return _m1

def main() -> None:
    println(classify(0))
    println(classify(1))
    println(classify(7))

