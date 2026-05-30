from __future__ import annotations
from dataclasses import dataclass
from typing import Union

@dataclass(frozen=True)
class Dot:
    pass

@dataclass(frozen=True)
class Move:
    _0: float
    _1: float

@dataclass(frozen=True)
class Box:
    w: float
    h: float

Shape = Union[Dot, Move, Box]

def area(s: Shape) -> float:
    match s:
        case Move(dx, dy):
            _m1 = (dx + dy)
        case Box(w=w, h=h):
            _m1 = (w * h)
        case _:
            _m1 = 0
    return _m1

def main() -> None:
    println(area(Move(3, 4)))
    println(area(Box(w=5, h=6)))
    println(area(Dot()))

