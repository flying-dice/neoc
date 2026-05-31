from __future__ import annotations

def main() -> None:
    acc = 0
    for i in range(0, 3):
        for j in range(0, 3):
            if (j == 1):
                continue
            if (j == 2):
                break
            acc = ((acc + (i * 10)) + j)
    println(acc)

