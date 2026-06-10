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

def twice(x: Doubler) -> float:
    return x.double()

def main() -> None:
    println(twice(Num(v=21)))

