from __future__ import annotations

def greet(name: str) -> str:
    return (("hello, " + name) + "!")

def main() -> None:
    println(greet("world"))

