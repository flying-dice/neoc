const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);
const __t = {};

__m.Counter = {};
__m.Counter.clone = function(self) {
  return { __struct: "Counter", value: self.value };
};
__t.Counter = ["Clone"];
function main() {
  let a = { __struct: "Counter", value: 5 };
  let b = __call(a, "clone");
  println(a.value);
  println(b.value);
}
