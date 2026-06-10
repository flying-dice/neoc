from __future__ import annotations
from dataclasses import dataclass
from typing import Protocol

class Show(Protocol):
    def show(self) -> str:
        ...

class Greet(Protocol):
    def greet(self) -> str:
        ...

@dataclass(frozen=True)
class Badger:
    name: str
    def show(self) -> str:
        return self.name
    def greet(self) -> str:
        return ("Hi, " + self.show())

def main() -> None:
    b = Badger(name="Bram")
    println(b.greet())

