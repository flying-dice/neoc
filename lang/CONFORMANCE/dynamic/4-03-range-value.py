from __future__ import annotations

def main() -> None:
    half = list(range(1, 4))
    incl = list(range(1, 4 + 1))
    sum = 0
    for x in half:
        sum = (sum + x)
    for y in incl:
        sum = (sum + y)
    println(sum)

