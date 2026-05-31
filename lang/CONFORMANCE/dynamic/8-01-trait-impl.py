from __future__ import annotations
from dataclasses import dataclass
from typing import Protocol

class Greet(Protocol):
    def greet(self) -> str:
        ...

@dataclass(frozen=True)
class Dog:
    name: str
    def greet(self) -> str:
        return (self.name + " says woof")

def main() -> None:
    d = Dog(name="Rex")
    println(d.greet())

