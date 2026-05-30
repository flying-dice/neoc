from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Point:
    x: float
    y: float
    def manhattan(self) -> float:
        return (self.x + self.y)

def main() -> None:
    p = Point(x=3, y=4)
    println(p.x)
    println(p.manhattan())

