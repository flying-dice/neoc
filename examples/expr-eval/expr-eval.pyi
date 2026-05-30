from dataclasses import dataclass
from typing import Union

@dataclass(frozen=True)
class VarId:
    _0: str

@dataclass(frozen=True)
class Add:
    ...
@dataclass(frozen=True)
class Sub:
    ...
@dataclass(frozen=True)
class Mul:
    ...
@dataclass(frozen=True)
class Div:
    ...
Op = Union[Add, Sub, Mul, Div]

@dataclass(frozen=True)
class Lit:
    _0: float
@dataclass(frozen=True)
class Var:
    _0: VarId
@dataclass(frozen=True)
class Bin:
    op: Op
    lhs: Expr
    rhs: Expr
Expr = Union[Lit, Var, Bin]

@dataclass(frozen=True)
class UnknownVar:
    _0: str
@dataclass(frozen=True)
class DivByZero:
    ...
EvalError = Union[UnknownVar, DivByZero]

@dataclass(frozen=True)
class Binding:
    name: str
    value: float

@dataclass(frozen=True)
class Env:
    bindings: list[Binding]
    def lookup(self, name: str) -> Option[float]: ...

@dataclass(frozen=True)
class Outcome:
    label: str
    value: float
    def outcome(self) -> str: ...
    def describe(self) -> str: ...
