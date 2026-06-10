from __future__ import annotations

__tests = {}

def checks_alpha() -> None:
    println("alpha-ran")

__tests["checks_alpha"] = checks_alpha
def checks_beta() -> None:
    println("beta-ran")

__tests["checks_beta"] = checks_beta
def main() -> None:
    checks_alpha()
    checks_beta()

