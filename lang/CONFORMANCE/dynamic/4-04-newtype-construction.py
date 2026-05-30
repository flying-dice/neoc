from __future__ import annotations
from dataclasses import dataclass

@dataclass(frozen=True)
class Wrapper:
    _0: float
    def get(self) -> float:
        match self:
            case Wrapper(n):
                pass
        return n

def main() -> None:
    w = Wrapper(42)
    println(w.get())
    match w:
        case Wrapper(m):
            pass
    println(m)

