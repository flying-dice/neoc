from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Point:
    x: float
    y: float

def main() -> None:
    Point(x=1, y=2)
    println(0)

