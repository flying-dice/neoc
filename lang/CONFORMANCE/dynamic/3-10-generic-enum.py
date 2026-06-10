from __future__ import annotations
from dataclasses import dataclass
from typing import Generic, TypeVar, Union

T = TypeVar("T")

@dataclass(frozen=True)
class Leaf:
    _0: T

@dataclass(frozen=True)
class Empty:
    pass

Tree = Union[Leaf, Empty]

def value(t: Tree[float]) -> float:
    match t:
        case Leaf(n):
            _m1 = n
        case Empty():
            _m1 = 0
    return _m1

def main() -> None:
    println(value(Leaf(5)))
    println(value(Empty()))

