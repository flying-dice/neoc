const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);

__m.Wrapper = {};
__m.Wrapper.get = function(self) {
  const __d1 = self;
  const n = __d1._0;
  return n;
}
function main() {
  let w = { __struct: "Wrapper", _0: 42 };
  println(__call(w, "get"));
  const __d2 = w;
  const m = __d2._0;
  println(m);
}
