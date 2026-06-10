from __future__ import annotations

def main() -> None:
    xs = [2, 4, 6, 8]
    sum = 0
    for x in xs:
        sum = (sum + x)
    println(sum)
    acc = 0
    for x in xs:
        acc = ((acc * 10) + x)
    println(acc)

