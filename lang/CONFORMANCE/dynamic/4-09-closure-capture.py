from __future__ import annotations
from typing import Callable

def adder(base: float) -> Callable[[float], float]:
    return (lambda x: (x + base))

def main() -> None:
    add10 = adder(10)
    println(add10(5))
    println(add10(32))

