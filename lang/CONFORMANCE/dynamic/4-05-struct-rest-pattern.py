from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Point:
    x: float
    y: float
    z: float

def classify(p: Point) -> float:
    match p:
        case Point(x=0, y=y):
            _m1 = y
        case Point(x=x):
            _m1 = x
    return _m1

def main() -> None:
    println(classify(Point(x=0, y=7, z=99)))
    println(classify(Point(x=5, y=8, z=99)))

