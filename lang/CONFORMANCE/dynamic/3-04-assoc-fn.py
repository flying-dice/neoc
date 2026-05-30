from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Counter:
    value: float
    @staticmethod
    def start(v: float) -> Counter:
        return Counter(value=v)
    def bump(self) -> float:
        return (self.value + 1)

def main() -> None:
    c = Counter.start(41)
    println(c.value)
    println(c.bump())

