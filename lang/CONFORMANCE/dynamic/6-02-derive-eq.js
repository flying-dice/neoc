const __t = {};

const Pair$ = {};
Pair$.eq = function(a, b) {
  return a.__struct === b.__struct && a.a === b.a && a.b === b.b;
};
__t.Pair = ["Eq"];
function main() {
  let p = { __struct: "Pair", a: 1, b: 2, __proto__: Pair$ };
  let q = { __struct: "Pair", a: 1, b: 2, __proto__: Pair$ };
  let r = { __struct: "Pair", a: 9, b: 2, __proto__: Pair$ };
  if (p.eq(p, q)) {
    println("eq");
  }
  return (() => {
    if ((!p.eq(p, r))) {
      println("ne");
    }
  })();
}
