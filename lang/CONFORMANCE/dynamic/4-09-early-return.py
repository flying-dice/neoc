from __future__ import annotations

def clamp(n: float) -> float:
    if (n < 0):
        return 0
    if (n > 100):
        return 100
    return n

def main() -> None:
    println(clamp((-5)))
    println(clamp(250))
    println(clamp(42))

