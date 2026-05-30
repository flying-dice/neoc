from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Dog:
    name: str
    def show(self) -> str:
        return self.name
    def describe(self) -> str:
        return self.show()

def main() -> None:
    d = Dog(name="Rex")
    println(d.describe())

