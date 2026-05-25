const __m = {};
const __call = (recv, name, ...args) => __m[recv._struct ?? recv._enum][name](recv, ...args);

__m.Box = {};
__m.Box.get = function(self) {
  return self.value;
}
function main() {
  let b = { _struct: "Box", value: 99 };
  println(__call(b, "get"));
}
