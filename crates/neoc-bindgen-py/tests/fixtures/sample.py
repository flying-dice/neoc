import os
from typing import Any, List


class Rgb:
    r: int
    g: int
    b: int


def lighten(color: str, amount: float) -> str: ...


def to_rgb(color: str) -> Rgb:
    return Rgb()


def grab(x) -> Any:
    return x


def merge(a: dict, b: dict) -> dict:
    return {**a, **b}


WHITE: str = "ffffff"

COUNT: int


_private: int = 1


def _hidden() -> None: ...
