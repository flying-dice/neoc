from __future__ import annotations
from dataclasses import dataclass, replace

@dataclass(frozen=True)
class Vec3:
    x: float
    y: float
    z: float

def main() -> None:
    v = Vec3(x=1, y=2, z=3)
    w = replace(v, x=10, z=30)
    println(w.x)
    println(w.y)
    println(w.z)

