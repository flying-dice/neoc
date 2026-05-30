import type { Result, Option } from "./neoc-lib";

interface VarId { _0: string; }
type Op = { __enum: "Op"; __tag: "Add" } | { __enum: "Op"; __tag: "Sub" } | { __enum: "Op"; __tag: "Mul" } | { __enum: "Op"; __tag: "Div" };
type Expr = { __enum: "Expr"; __tag: "Lit"; _0: number } | { __enum: "Expr"; __tag: "Var"; _0: VarId } | { __enum: "Expr"; __tag: "Bin"; op: Op; lhs: Expr; rhs: Expr };
type EvalError = { __enum: "EvalError"; __tag: "UnknownVar"; _0: string } | { __enum: "EvalError"; __tag: "DivByZero" };
interface Binding { name: string; value: number; }
interface Env { bindings: Binding[]; }
interface Outcome { label: string; value: number; }

const __t = {};
import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

const EvalError$ = {};
EvalError$.display = function() {
  if (this.__tag === "UnknownVar") {
    return String(this._0);
  }
  if (this.__tag === "DivByZero") {
    return "DivByZero";
  }
};
__t.EvalError = ["Display"];
const Binding$ = {};
Binding$.clone = function() {
  return { __struct: "Binding", name: this.name, value: this.value, __proto__: Binding$ };
};
Binding$.debug = function() {
  return "Binding { " + "name = " + String(this.name) + ", " + "value = " + String(this.value) + " }";
};
Binding$.eq = function(a, b) {
  return a.__struct === b.__struct && a.name === b.name && a.value === b.value;
};
__t.Binding = ["Clone", "Debug", "Eq"];
const Env$ = {};
Env$.lookup = function(name: string) {
  for (const b of this.bindings) {
    if ((b.name === name)) {
      return Some(b.value);
    }
  }
  return None;
}
function idiv(a: number, b: number): number {
  let q = 0;
  let r = a;
  while ((r >= b)) {
    r = (r - b);
    q = (q + 1);
  }
  return q;
}
function apply(op: Op, a: number, b: number): Result<number, EvalError> {
  return (() => {
    const __s1 = op;
    if (__s1.__tag === "Add") {
      return Ok((a + b));
    }
    if (__s1.__tag === "Sub") {
      return Ok((a - b));
    }
    if (__s1.__tag === "Mul") {
      return Ok((a * b));
    }
    if (__s1.__tag === "Div") {
      return (() => {
        return (() => {
          const __s2 = b;
          if (__s2 === 0) {
            return Err({ __enum: "EvalError", __tag: "DivByZero", __proto__: EvalError$ });
          }
          if (true) {
            return Ok(idiv(a, b));
          }
        })();
      })();
    }
  })();
}
function evaluate(e: Expr, env: Env): Result<number, EvalError> {
  return (() => {
    const __s3 = e;
    if (__s3.__tag === "Lit") {
      const n = __s3._0;
      return Ok(n);
    }
    if (__s3.__tag === "Var") {
      const id = __s3._0;
      return (() => {
        const __d4 = id;
        const name = __d4._0;
        return env.lookup(name).ok_or({ __enum: "EvalError", __tag: "UnknownVar", _0: name, __proto__: EvalError$ });
      })();
    }
    if (__s3.__tag === "Bin") {
      const op = __s3.op;
      const lhs = __s3.lhs;
      const rhs = __s3.rhs;
      return (() => {
        const __try_0 = evaluate(lhs, env);
        if (__try_0.__tag === "Err" || __try_0.__tag === "None") return __try_0;
        let a = __try_0._0;
        const __try_1 = evaluate(rhs, env);
        if (__try_1.__tag === "Err" || __try_1.__tag === "None") return __try_1;
        let b = __try_1._0;
        return apply(op, a, b);
      })();
    }
  })();
}
function shape(e: Expr): string {
  return (() => {
    const __s5 = e;
    if (__s5.__tag === "Bin" && __s5.op.__tag === "Div" && __s5.lhs.__tag === "Lit" && __s5.lhs._0 === 0) {
      const rhs = __s5.rhs;
      return "0 / _";
    }
    if (__s5.__tag === "Bin" && __s5.op.__tag === "Div") {
      const lhs = __s5.lhs;
      const rhs = __s5.rhs;
      return "_ / _";
    }
    if (__s5.__tag === "Bin") {
      const op = __s5.op;
      const lhs = __s5.lhs;
      const rhs = __s5.rhs;
      return "binary op";
    }
    if (__s5.__tag === "Lit") {
      const n = __s5._0;
      return "literal";
    }
    if (true) {
      return "variable";
    }
  })();
}
function band(n: number): string {
  return (() => {
    const __s6 = n;
    if (__s6 === 0) {
      return "zero";
    }
    if (((__s6 === 1) || (__s6 === 2) || (__s6 === 3))) {
      return "low";
    }
    if (true) {
      const n = __s6;
      if ((n > 100)) {
        return "high";
      }
    }
    if (true) {
      return "mid";
    }
  })();
}
const Outcome$ = {};
Outcome$.display = function() {
  return String(this.label) + ", " + String(this.value);
};
__t.Outcome = ["Display", "Report"];
Outcome$.outcome = function() {
  return ((this.label + " = ") + render(this.value));
}
Outcome$.describe = function() {
  return ("result: " + this.outcome());
}
function first<T>(a: T, b: T): T {
  return a;
}
function render(n: number): string {
  return (() => {
    if ((n === 0)) {
      return "0";
    } else {
      let neg = (n < 0);
      let m = (() => {
        if (neg) {
          return (0 - n);
        } else {
          return n;
        }
      })();
      let out = "";
      while ((m > 0)) {
        let d = (m % 10);
        out = (digit(d) + out);
        m = idiv(m, 10);
      }
      return (() => {
        if (neg) {
          return ("-" + out);
        } else {
          return out;
        }
      })();
    }
  })();
}
function digit(d: number): string {
  return (() => {
    const __s7 = d;
    if (__s7 === 0) {
      return "0";
    }
    if (__s7 === 1) {
      return "1";
    }
    if (__s7 === 2) {
      return "2";
    }
    if (__s7 === 3) {
      return "3";
    }
    if (__s7 === 4) {
      return "4";
    }
    if (__s7 === 5) {
      return "5";
    }
    if (__s7 === 6) {
      return "6";
    }
    if (__s7 === 7) {
      return "7";
    }
    if (__s7 === 8) {
      return "8";
    }
    if (__s7 === 9) {
      return "9";
    }
    if (true) {
      return "?";
    }
  })();
}
function run(label: string, e: Expr, env: Env): string {
  return (() => {
    const __s8 = evaluate(e, env);
    if (__s8.__tag === "Ok") {
      const v = __s8._0;
      return (() => {
        let o = { __struct: "Outcome", label: label, value: v, __proto__: Outcome$ };
        return o.describe();
      })();
    }
    if (__s8.__tag === "Err") {
      const err = __s8._0;
      return ((label + " failed: ") + err.display());
    }
  })();
}
function main(): void {
  let env = { __struct: "Env", bindings: [{ __struct: "Binding", name: "x", value: 6, __proto__: Binding$ }, { __struct: "Binding", name: "y", value: 7, __proto__: Binding$ }, { __struct: "Binding", name: "z", value: 0, __proto__: Binding$ }], __proto__: Env$ };
  let e1 = { __enum: "Expr", __tag: "Bin", op: { __enum: "Op", __tag: "Sub" }, lhs: { __enum: "Expr", __tag: "Bin", op: { __enum: "Op", __tag: "Mul" }, lhs: { __enum: "Expr", __tag: "Var", _0: { __struct: "VarId", _0: "x" } }, rhs: { __enum: "Expr", __tag: "Var", _0: { __struct: "VarId", _0: "y" } } }, rhs: { __enum: "Expr", __tag: "Lit", _0: 2 } };
  let e2 = { __enum: "Expr", __tag: "Bin", op: { __enum: "Op", __tag: "Div" }, lhs: { __enum: "Expr", __tag: "Var", _0: { __struct: "VarId", _0: "x" } }, rhs: { __enum: "Expr", __tag: "Var", _0: { __struct: "VarId", _0: "z" } } };
  let e3 = { __enum: "Expr", __tag: "Bin", op: { __enum: "Op", __tag: "Add" }, lhs: { __enum: "Expr", __tag: "Var", _0: { __struct: "VarId", _0: "w" } }, rhs: { __enum: "Expr", __tag: "Lit", _0: 1 } };
  println(run("(x * y) - 2", e1, env));
  println(run("x / z", e2, env));
  println(run("w + 1", e3, env));
  let names = ["x", "y", "missing"];
  let probe = (name) => env.lookup(name).map((v) => (v + 100)).unwrap_or(0);
  for (const name of names) {
    println(((name + " -> ") + render(probe(name))));
  }
  let zero_div = { __enum: "Expr", __tag: "Bin", op: { __enum: "Op", __tag: "Div" }, lhs: { __enum: "Expr", __tag: "Lit", _0: 0 }, rhs: { __enum: "Expr", __tag: "Var", _0: { __struct: "VarId", _0: "x" } } };
  println(("shape(0 / x) = " + shape(zero_div)));
  println(("band(2) = " + band(2)));
  println(("band(250) = " + band(250)));
  let picked = first(11, 22);
  let parity = (() => {
    if (((picked % 2) === 0)) {
      return "even";
    } else {
      return "odd";
    }
  })();
  println((((("first(11, 22) = " + render(picked)) + " (") + parity) + ")"));
  let a = { __struct: "Binding", name: "x", value: 6, __proto__: Binding$ };
  let b = a.clone();
  println(("clone eq: " + boolstr(a.eq(a, b))));
  println(("debug: " + b.debug()));
}
function boolstr(t: boolean): string {
  return (() => {
    if (t) {
      return "true";
    } else {
      return "false";
    }
  })();
}
