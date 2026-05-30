interface Wrapper { _0: number; }

const Wrapper$ = {};
Wrapper$.get = function() {
  const __d1 = this;
  const n = __d1._0;
  return n;
}
function main(): void {
  let w = { __struct: "Wrapper", _0: 42, __proto__: Wrapper$ };
  println(w.get());
  const __d2 = w;
  const m = __d2._0;
  println(m);
}
