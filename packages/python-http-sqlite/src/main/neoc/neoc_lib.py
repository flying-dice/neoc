from __future__ import annotations

from dataclasses import dataclass
from typing import Callable, Generic, TypeVar, Union

T = TypeVar("T")
E = TypeVar("E")
U = TypeVar("U")
F = TypeVar("F")


@dataclass(frozen=True)
class Ok(Generic[T]):
    _0: T

    def is_ok(self) -> bool:
        return True

    def is_err(self) -> bool:
        return False

    def unwrap(self) -> T:
        return self._0

    def unwrap_err(self) -> object:
        raise Exception("called unwrap_err on an Ok value")

    def map(self, f: Callable[[T], U]) -> "Result[U, object]":
        return Ok(f(self._0))

    def map_err(self, f: Callable[[object], F]) -> "Result[T, F]":
        return self


@dataclass(frozen=True)
class Err(Generic[E]):
    _0: E

    def is_ok(self) -> bool:
        return False

    def is_err(self) -> bool:
        return True

    def unwrap(self) -> object:
        raise Exception("called unwrap on an Err value")

    def unwrap_err(self) -> E:
        return self._0

    def map(self, f: Callable[[object], U]) -> "Result[U, E]":
        return self

    def map_err(self, f: Callable[[E], F]) -> "Result[object, F]":
        return Err(f(self._0))


Result = Union[Ok, Err]


@dataclass(frozen=True)
class Some(Generic[T]):
    _0: T

    def is_some(self) -> bool:
        return True

    def is_none(self) -> bool:
        return False

    def unwrap(self) -> T:
        return self._0

    def unwrap_or(self, default: T) -> T:
        return self._0

    def map(self, f: Callable[[T], U]) -> "Option[U]":
        return Some(f(self._0))

    def ok_or(self, err: E) -> "Result[T, E]":
        return Ok(self._0)


@dataclass(frozen=True)
class None_:
    def is_some(self) -> bool:
        return False

    def is_none(self) -> bool:
        return True

    def unwrap(self) -> object:
        raise Exception("called unwrap on a None value")

    def unwrap_or(self, default: T) -> T:
        return default

    def map(self, f: Callable[[object], U]) -> "Option[U]":
        return self

    def ok_or(self, err: E) -> "Result[object, E]":
        return Err(err)


Option = Union[Some, None_]


__all__ = ["Ok", "Err", "Result", "Some", "None_", "Option"]
