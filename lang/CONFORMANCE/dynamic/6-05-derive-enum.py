from __future__ import annotations
from dataclasses import dataclass, replace
from typing import Union

@dataclass(frozen=True)
class Pulse:
    _0: float
    def clone(self):
        return replace(self)
    def debug(self) -> str:
        return "Pulse(" + str(self._0) + ")"
    def display(self) -> str:
        return str(self._0)

@dataclass(frozen=True)
class Idle:
    def clone(self):
        return replace(self)
    def debug(self) -> str:
        return "Idle"
    def display(self) -> str:
        return "Idle"

Signal = Union[Pulse, Idle]

def main() -> None:
    a = Pulse(7)
    b = a.clone()
    if (a == b):
        println("clone-eq")
    c = Idle()
    if (not (a == c)):
        println("tag-ne")
    d = Pulse(9)
    if (not (a == d)):
        println("payload-ne")
    println(a.debug())
    println(c.debug())
    println(a.display())
    println(c.display())

