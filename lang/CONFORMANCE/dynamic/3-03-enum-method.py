from __future__ import annotations
from dataclasses import dataclass
from typing import Union

@dataclass(frozen=True)
class N:
    def name(self) -> str:
        match self:
            case N():
                _m1 = "north"
            case S():
                _m1 = "south"
            case E():
                _m1 = "east"
            case W():
                _m1 = "west"
        return _m1

@dataclass(frozen=True)
class S:
    def name(self) -> str:
        match self:
            case N():
                _m2 = "north"
            case S():
                _m2 = "south"
            case E():
                _m2 = "east"
            case W():
                _m2 = "west"
        return _m2

@dataclass(frozen=True)
class E:
    def name(self) -> str:
        match self:
            case N():
                _m3 = "north"
            case S():
                _m3 = "south"
            case E():
                _m3 = "east"
            case W():
                _m3 = "west"
        return _m3

@dataclass(frozen=True)
class W:
    def name(self) -> str:
        match self:
            case N():
                _m4 = "north"
            case S():
                _m4 = "south"
            case E():
                _m4 = "east"
            case W():
                _m4 = "west"
        return _m4

Dir = Union[N, S, E, W]

def main() -> None:
    d = E()
    println(d.name())

