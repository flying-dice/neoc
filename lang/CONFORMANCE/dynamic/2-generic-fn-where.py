from __future__ import annotations
from dataclasses import dataclass
from typing import Protocol

class Doubler(Protocol):
    def double(self) -> float:
        ...

@dataclass(frozen=True)
class Num:
    v: float
    def double(self) -> float:
        return (self.v + self.v)

def combine(a: Doubler, b: Doubler) -> float:
    return (a.double() + b.double())

def main() -> None:
    println(combine(Num(v=21), Num(v=5)))

