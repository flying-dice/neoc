from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Point:
    x: float
    y: float
    def display(self) -> str:
        return str(self.x) + ", " + str(self.y)

def main() -> None:
    p = Point(x=1, y=2)
    println(p.display())

