from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Circle:
    r: float
    def draw(self) -> str:
        return "O"

@dataclass(frozen=True)
class Square:
    side: float
    def draw(self) -> str:
        return "[]"

def render(shape: Drawable) -> str:
    return shape.draw()

def main() -> None:
    println(render(Circle(r=1)))
    println(render(Square(side=2)))

