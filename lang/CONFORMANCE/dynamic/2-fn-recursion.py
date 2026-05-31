from __future__ import annotations

def fib(n: float) -> float:
    return (n if (n < 2) else (fib((n - 1)) + fib((n - 2))))

def main() -> None:
    println(fib(10))

