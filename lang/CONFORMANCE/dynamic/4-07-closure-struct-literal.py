from __future__ import annotations
from dataclasses import dataclass
from typing import Union

@dataclass(frozen=True)
class Point:
    x: float
    y: float

@dataclass(frozen=True)
class Square:
    side: float

Shape = Union[Square]

def main() -> None:
    mk = (lambda n: Point(x=n, y=n))
    p = mk(7)
    println(p.x)
    println(p.y)
    sq = (lambda n: Square(side=n))
    s = sq(5)
    match s:
        case Square(side=side):
            _m1 = println(side)
    return _m1

