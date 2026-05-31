from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Pair:
    a: float
    b: float

def main() -> None:
    p = Pair(a=1, b=2)
    q = Pair(a=1, b=2)
    r = Pair(a=9, b=2)
    if (p == q):
        println("eq")
    if (not (p == r)):
        println("ne")

