from __future__ import annotations

def loud(tag: str) -> bool:
    println(tag)
    return True

def main() -> None:
    (False and loud("and-skipped"))
    (True or loud("or-skipped"))
    (True and loud("and-run"))
    (False or loud("or-run"))

