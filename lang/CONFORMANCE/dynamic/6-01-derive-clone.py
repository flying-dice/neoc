from __future__ import annotations
from dataclasses import dataclass, replace

@dataclass(frozen=True)
class Counter:
    value: float
    def clone(self):
        return replace(self)

def main() -> None:
    a = Counter(value=5)
    b = a.clone()
    println(a.value)
    println(b.value)

