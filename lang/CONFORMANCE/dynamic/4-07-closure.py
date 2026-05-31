from __future__ import annotations
from typing import Callable

def apply(f: Callable[[float], float], x: float) -> float:
    return f(x)

def main() -> None:
    double = (lambda n: (n * 2))
    println(apply(double, 21))

