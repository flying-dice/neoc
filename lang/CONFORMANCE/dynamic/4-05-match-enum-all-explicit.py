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
class Circle:
    radius: float

Shape = Union[Dot, Move, Circle]

def describe(s: Shape) -> str:
    match s:
        case Dot():
            _m1 = "dot"
        case Move(a, b):
            _m1 = "move"
        case Circle(radius=radius):
            _m1 = "circle"
    return _m1

def main() -> None:
    print(describe(Dot()))
    print(describe(Move(1, 2)))
    print(describe(Circle(radius=5)))

