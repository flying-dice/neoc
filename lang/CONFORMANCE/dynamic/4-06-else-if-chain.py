from __future__ import annotations

def grade(n: float) -> str:
    if (n >= 90):
        _if1 = "A"
    elif (n >= 80):
        _if1 = "B"
    elif (n >= 70):
        _if1 = "C"
    else:
        _if1 = "F"
    return _if1

def main() -> None:
    println(grade(95))
    println(grade(82))
    println(grade(40))

