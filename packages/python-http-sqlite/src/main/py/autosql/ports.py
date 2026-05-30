from __future__ import annotations
from dataclasses import dataclass
from typing import Union
from ..neoc_lib import Result, Option, Ok, Err, Some, None_

@dataclass(frozen=True)
class Int:
    _0: float

@dataclass(frozen=True)
class Real:
    _0: float

@dataclass(frozen=True)
class Text:
    _0: str

@dataclass(frozen=True)
class Bool:
    _0: bool

@dataclass(frozen=True)
class Null:
    pass

SqlValue = Union[Int, Real, Text, Bool, Null]

def bind_opt_int(value: Option[float]) -> SqlValue:
    match value:
        case Some(inner):
            _m1 = Int(inner)
        case None_():
            _m1 = Null()
    return _m1

def bind_opt_real(value: Option[float]) -> SqlValue:
    match value:
        case Some(inner):
            _m2 = Real(inner)
        case None_():
            _m2 = Null()
    return _m2

def bind_opt_text(value: Option[str]) -> SqlValue:
    match value:
        case Some(inner):
            _m3 = Text(inner)
        case None_():
            _m3 = Null()
    return _m3

def bind_opt_bool(value: Option[bool]) -> SqlValue:
    match value:
        case Some(inner):
            _m4 = Bool(inner)
        case None_():
            _m4 = Null()
    return _m4

def decode_number(value: SqlValue, label: str) -> Result[float, str]:
    match value:
        case Int(inner) | Real(inner):
            _m5 = Ok(inner)
        case _:
            _m5 = Err(label)
    return _m5

def decode_opt_number(value: SqlValue, label: str) -> Result[Option[float], str]:
    match value:
        case Null():
            _m6 = Ok(None_())
        case Int(inner) | Real(inner):
            _m6 = Ok(Some(inner))
        case _:
            _m6 = Err(label)
    return _m6

def decode_string(value: SqlValue, label: str) -> Result[str, str]:
    match value:
        case Text(inner):
            _m7 = Ok(inner)
        case _:
            _m7 = Err(label)
    return _m7

def decode_opt_string(value: SqlValue, label: str) -> Result[Option[str], str]:
    match value:
        case Null():
            _m8 = Ok(None_())
        case Text(inner):
            _m8 = Ok(Some(inner))
        case _:
            _m8 = Err(label)
    return _m8

def decode_bool(value: SqlValue, label: str) -> Result[bool, str]:
    match value:
        case Bool(inner):
            _m9 = Ok(inner)
        case _:
            _m9 = Err(label)
    return _m9

def decode_opt_bool(value: SqlValue, label: str) -> Result[Option[bool], str]:
    match value:
        case Null():
            _m10 = Ok(None_())
        case Bool(inner):
            _m10 = Ok(Some(inner))
        case _:
            _m10 = Err(label)
    return _m10


__all__ = ["SqlValue", "bind_opt_int", "bind_opt_real", "bind_opt_text", "bind_opt_bool", "decode_number", "decode_opt_number", "decode_string", "decode_opt_string", "decode_bool", "decode_opt_bool"]
