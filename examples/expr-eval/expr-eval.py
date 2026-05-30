from __future__ import annotations
from dataclasses import dataclass, replace
from typing import Union
from .neoc_lib import Result, Option, Ok, Err, Some, None_

@dataclass(frozen=True)
class VarId:
    _0: str

@dataclass(frozen=True)
class Add:
    pass

@dataclass(frozen=True)
class Sub:
    pass

@dataclass(frozen=True)
class Mul:
    pass

@dataclass(frozen=True)
class Div:
    pass

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
    def display(self) -> str:
        return str(self._0)

@dataclass(frozen=True)
class DivByZero:
    def display(self) -> str:
        return "DivByZero"

EvalError = Union[UnknownVar, DivByZero]

@dataclass(frozen=True)
class Binding:
    name: str
    value: float
    def clone(self):
        return replace(self)
    def debug(self) -> str:
        return "Binding { " + "name = " + str(self.name) + ", " + "value = " + str(self.value) + " }"

@dataclass(frozen=True)
class Env:
    bindings: list[Binding]
    def lookup(self, name: str) -> Option[float]:
        for b in self.bindings:
            if (b.name == name):
                return Some(b.value)
        return None_()

def idiv(a: float, b: float) -> float:
    q = 0
    r = a
    while (r >= b):
        r = (r - b)
        q = (q + 1)
    return q

def apply(op: Op, a: float, b: float) -> Result[float, EvalError]:
    match op:
        case Add():
            _m1 = Ok((a + b))
        case Sub():
            _m1 = Ok((a - b))
        case Mul():
            _m1 = Ok((a * b))
        case Div():
            match b:
                case 0:
                    _m2 = Err(DivByZero())
                case _:
                    _m2 = Ok(idiv(a, b))
            _blk3 = _m2
            _m1 = _blk3
    return _m1

def evaluate(e: Expr, env: Env) -> Result[float, EvalError]:
    match e:
        case Lit(n):
            _m4 = Ok(n)
        case Var(id):
            match id:
                case VarId(name):
                    pass
            _blk5 = env.lookup(name).ok_or(UnknownVar(name))
            _m4 = _blk5
        case Bin(op=op, lhs=lhs, rhs=rhs):
            _try0 = evaluate(lhs, env)
            if isinstance(_try0, (Err, None_)):
                return _try0
            a = _try0._0
            _try1 = evaluate(rhs, env)
            if isinstance(_try1, (Err, None_)):
                return _try1
            b = _try1._0
            _blk6 = apply(op, a, b)
            _m4 = _blk6
    return _m4

def shape(e: Expr) -> str:
    match e:
        case Bin(op=Div(), lhs=Lit(0), rhs=rhs):
            _m7 = "0 / _"
        case Bin(op=Div(), lhs=lhs, rhs=rhs):
            _m7 = "_ / _"
        case Bin(op=op, lhs=lhs, rhs=rhs):
            _m7 = "binary op"
        case Lit(n):
            _m7 = "literal"
        case _:
            _m7 = "variable"
    return _m7

def band(n: float) -> str:
    match n:
        case 0:
            _m8 = "zero"
        case 1 | 2 | 3:
            _m8 = "low"
        case n if (n > 100):
            _m8 = "high"
        case _:
            _m8 = "mid"
    return _m8

@dataclass(frozen=True)
class Outcome:
    label: str
    value: float
    def outcome(self) -> str:
        return ((self.label + " = ") + render(self.value))
    def describe(self) -> str:
        return ("result: " + self.outcome())
    def display(self) -> str:
        return str(self.label) + ", " + str(self.value)

def first(a: T, b: T) -> T:
    return a

def render(n: float) -> str:
    if (n == 0):
        _if9 = "0"
    else:
        neg = (n < 0)
        m = ((0 - n) if neg else n)
        out = ""
        while (m > 0):
            d = (m % 10)
            out = (digit(d) + out)
            m = idiv(m, 10)
        _if9 = (("-" + out) if neg else out)
    return _if9

def digit(d: float) -> str:
    match d:
        case 0:
            _m10 = "0"
        case 1:
            _m10 = "1"
        case 2:
            _m10 = "2"
        case 3:
            _m10 = "3"
        case 4:
            _m10 = "4"
        case 5:
            _m10 = "5"
        case 6:
            _m10 = "6"
        case 7:
            _m10 = "7"
        case 8:
            _m10 = "8"
        case 9:
            _m10 = "9"
        case _:
            _m10 = "?"
    return _m10

def run(label: str, e: Expr, env: Env) -> str:
    match evaluate(e, env):
        case Ok(v):
            o = Outcome(label=label, value=v)
            _blk12 = o.describe()
            _m11 = _blk12
        case Err(err):
            _m11 = ((label + " failed: ") + err.display())
    return _m11

def main() -> None:
    env = Env(bindings=[Binding(name="x", value=6), Binding(name="y", value=7), Binding(name="z", value=0)])
    e1 = Bin(op=Sub(), lhs=Bin(op=Mul(), lhs=Var(VarId("x")), rhs=Var(VarId("y"))), rhs=Lit(2))
    e2 = Bin(op=Div(), lhs=Var(VarId("x")), rhs=Var(VarId("z")))
    e3 = Bin(op=Add(), lhs=Var(VarId("w")), rhs=Lit(1))
    println(run("(x * y) - 2", e1, env))
    println(run("x / z", e2, env))
    println(run("w + 1", e3, env))
    names = ["x", "y", "missing"]
    probe = (lambda name: env.lookup(name).map((lambda v: (v + 100))).unwrap_or(0))
    for name in names:
        println(((name + " -> ") + render(probe(name))))
    zero_div = Bin(op=Div(), lhs=Lit(0), rhs=Var(VarId("x")))
    println(("shape(0 / x) = " + shape(zero_div)))
    println(("band(2) = " + band(2)))
    println(("band(250) = " + band(250)))
    picked = first(11, 22)
    parity = ("even" if ((picked % 2) == 0) else "odd")
    println((((("first(11, 22) = " + render(picked)) + " (") + parity) + ")"))
    a = Binding(name="x", value=6)
    b = a.clone()
    println(("clone eq: " + boolstr((a == b))))
    println(("debug: " + b.debug()))

def boolstr(t: bool) -> str:
    return ("true" if t else "false")

