from __future__ import annotations
from dataclasses import dataclass
from typing import Generic, TypeVar

T = TypeVar("T")

@dataclass(frozen=True)
class Box(Generic[T]):
    value: T
    def get(self) -> T:
        return self.value

def main() -> None:
    b = Box(value=99)
    println(b.get())

