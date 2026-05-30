from __future__ import annotations
from dataclasses import dataclass, replace

@dataclass(frozen=True)
class Point:
    x: float
    y: float

def main() -> None:
    p = Point(x=1, y=2)
    q = replace(p, x=9)
    println(q.x)
    println(q.y)

