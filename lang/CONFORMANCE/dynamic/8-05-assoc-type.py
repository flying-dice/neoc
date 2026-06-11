from __future__ import annotations
from dataclasses import dataclass
from typing import Any, Protocol

class Container(Protocol):
    def peek(self) -> Any:
        ...

@dataclass(frozen=True)
class Bin:
    value: float
    def peek(self) -> float:
        return self.value

def main() -> None:
    b = Bin(value=7)
    println(b.peek())

