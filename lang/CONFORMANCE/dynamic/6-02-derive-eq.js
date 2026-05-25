const __m = {};
const __call = (recv, name, ...args) => __m[recv._struct ?? recv._enum][name](recv, ...args);

__m.Pair = {};
__m.Pair.eq = function(a, b) {
  return a._struct === b._struct && a.a === b.a && a.b === b.b;
};
function main() {
  let p = { _struct: "Pair", a: 1, b: 2 };
  let q = { _struct: "Pair", a: 1, b: 2 };
  let r = { _struct: "Pair", a: 9, b: 2 };
  if (__call(p, "eq", q)) {
    println("eq");
  }
  return (() => {
    if ((!__call(p, "eq", r))) {
      println("ne");
    }
  })();
}
