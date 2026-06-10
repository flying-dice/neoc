from __future__ import annotations

def add(a: float, b: float) -> float:
    return (a + b)

def main() -> None:
    println(add(2, 3))
    println(add(10, 20))


__all__ = ["add"]
