interface Wrapper { _0: number; get(): number; }

const Wrapper$: Record<string, Function> = {};
Wrapper$.get = function(this: Wrapper) {
  const __d1 = this;
  const n = __d1._0;
  return n;
}
function main(): void {
  let w = { __struct: "Wrapper", _0: 42, __proto__: Wrapper$ } as unknown as Wrapper;
  println(w.get());
  const __d2 = w;
  const m = __d2._0;
  println(m);
}
