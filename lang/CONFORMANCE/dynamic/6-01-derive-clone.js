const __m = {};
const __call = (recv, name, ...args) => __m[recv._struct ?? recv._enum][name](recv, ...args);

__m.Counter = {};
__m.Counter.clone = function(self) {
  return { _struct: "Counter", value: self.value };
};
function main() {
  let a = { _struct: "Counter", value: 5 };
  let b = __call(a, "clone");
  println(a.value);
  println(b.value);
}
