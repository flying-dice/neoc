from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Sun:
    def shine(self) -> str:
        return "day"

@dataclass(frozen=True)
class Moon:
    def shine(self) -> str:
        return "night"

def main() -> None:
    a = Sun()
    b = Moon()
    println(a.shine())
    println(b.shine())

