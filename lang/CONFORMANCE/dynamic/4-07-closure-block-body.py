from __future__ import annotations

def main() -> None:
    def _closure1(n):
        doubled = (n * 2)
        return (doubled + 1)
    stepped = _closure1
    println(stepped(20))
    println(stepped(0))

