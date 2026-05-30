from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Point:
    x: float
    y: float

def main() -> None:
    p = Point(x=3, y=4)
    match p:
        case Point(x=x, y=y):
            pass
    println((x + y))

