# neoc examples

Cohesive, runnable neoc programs, each built to all five targets so every
backend has a substantial sample of neoc-generated code.

## `expr-eval` — arithmetic expression evaluator

A recursive `Expr` tree (literals, named variables, binary operators) evaluated
against an environment of variable bindings. Lookup failures and division by
zero flow back as a `Result` — the evaluator reports them instead of crashing.
One `.neoc` source compiles unchanged to Lua, JavaScript, TypeScript, Luau, and
Python and prints byte-identical output on each. It exercises a broad slice of
the language: a newtype (`VarId`) and a fielded `struct`; an `enum` with both
tuple- and struct-payload variants; a `trait` with an `impl`; a generic
function; `match` with a nested pattern, a guard, and an or-pattern; `if`/`else`
as a value; `Option`/`Result` with `?`, `ok_or`, `map`, `unwrap_or`; a closure
capturing its environment; `for`-over-list and `while` loops; and
`#[derive(Clone, Debug, Eq, Display)]`. Output crosses the host boundary through
a single `Unknown`-typed `println`, the one seam the runtime supplies.

### Build (all five targets)

```sh
neoc build examples/expr-eval/expr-eval.neoc --target lua
neoc build examples/expr-eval/expr-eval.neoc --target js
neoc build examples/expr-eval/expr-eval.neoc --target ts
neoc build examples/expr-eval/expr-eval.neoc --target luau
neoc build examples/expr-eval/expr-eval.neoc --target py
```

Each writes a sibling module (`expr-eval.lua` / `.js` / `.ts` / `.luau` /
`.py`). The JS, TS, and Python builds also emit a shared runtime and typings
sidecar (`neoc-lib.js`, `neoc-lib.d.ts`, `neoc-lib.d.lua`, `neoc_lib.py`,
`neoc_lib.pyi`, `expr-eval.d.ts`, `expr-eval.pyi`); keep those beside the module
— they are part of the buildable output.

### Run

The emitted modules are libraries: each defines `main` but does not call it, and
output goes through `println`, which the host supplies. Each runner below wires
`println` to the target's print and invokes `main`.

```sh
cd examples/expr-eval

# lua: alias println to print, append a main() call
{ echo 'println = print'; cat expr-eval.lua; echo 'main()'; } | lua

# luau: self-contained module; same prelude + driver
{ echo 'println = print'; cat expr-eval.luau; echo 'main()'; } > /tmp/run.luau && luau /tmp/run.luau

# js: ESM; route println through console.log, then call main() (neoc-lib.js must sit beside it)
{ printf 'globalThis.println=(x)=>console.log(x);\n'; cat expr-eval.js; echo 'main();'; } > /tmp/run.mjs \
  && cp neoc-lib.js /tmp/ && node /tmp/run.mjs

# ts: run under bun, same shape (neoc-lib.js beside it)
{ printf 'globalThis.println=(x)=>console.log(x);\n'; cat expr-eval.ts; echo 'main();'; } > /tmp/run.ts \
  && cp neoc-lib.js /tmp/ && bun /tmp/run.ts

# py: run as a package member with neoc_lib.py, injecting println (renders bool as true/false)
python3 - <<'PY'
import builtins, shutil, sys, pathlib
d = pathlib.Path('/tmp/exprpkg'); pkg = d / 'genpkg'; pkg.mkdir(parents=True, exist_ok=True)
(pkg / '__init__.py').write_text('')
shutil.copy('neoc_lib.py', pkg / 'neoc_lib.py')
shutil.copy('expr-eval.py', pkg / 'case.py')
def _fmt(x): return ('true' if x else 'false') if isinstance(x, bool) else str(x)
builtins.println = lambda x: print(_fmt(x))
sys.path.insert(0, str(d))
from genpkg.case import main; main()
PY
```

All five print:

```
result: (x * y) - 2 = 40
x / z failed: DivByZero
w + 1 failed: w
x -> 106
y -> 107
missing -> 0
shape(0 / x) = 0 / _
band(2) = low
band(250) = high
first(11, 22) = 11 (odd)
clone eq: true
debug: Binding { name = x, value = 6 }
```
