from __future__ import annotations

def classify(n: float) -> str:
    match n:
        case 0:
            _m1 = "zero"
        case 1 | 2 | 3:
            _m1 = "small"
        case x if (x > 100):
            _m1 = "huge"
        case _:
            _m1 = "other"
    return _m1

def main() -> None:
    println(classify(0))
    println(classify(2))
    println(classify(200))
    println(classify(50))

