from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Point:
    x: float
    y: float
    def debug(self) -> str:
        return "Point { " + "x = " + str(self.x) + ", " + "y = " + str(self.y) + " }"

def main() -> None:
    p = Point(x=1, y=2)
    println(p.debug())

