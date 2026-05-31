from __future__ import annotations
from dataclasses import dataclass
from typing import Protocol

class Show(Protocol):
    def show(self) -> str:
        ...

class Describe(Protocol):
    def describe(self) -> str:
        ...

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

