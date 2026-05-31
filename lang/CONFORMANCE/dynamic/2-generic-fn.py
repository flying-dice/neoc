from __future__ import annotations
from typing import TypeVar

T = TypeVar("T")

def first(a: T, b: T) -> T:
    return a

def main() -> None:
    println(first(7, 9))

