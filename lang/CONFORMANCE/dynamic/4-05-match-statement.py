from __future__ import annotations

def classify(n: float) -> None:
    match n:
        case 0:
            _m1 = println("zero")
        case 1:
            _m1 = println("one")
        case _:
            _m1 = println("many")
    return _m1

def main() -> None:
    classify(0)
    classify(1)
    classify(7)

