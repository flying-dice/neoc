from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Dog:
    name: str
    def greet(self) -> str:
        return (self.name + " says woof")

def main() -> None:
    d = Dog(name="Rex")
    println(d.greet())

