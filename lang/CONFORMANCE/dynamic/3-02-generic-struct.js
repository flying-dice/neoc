const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);

__m.Box = {};
__m.Box.get = function(self) {
  return self.value;
}
function main() {
  let b = { __struct: "Box", value: 99 };
  println(__call(b, "get"));
}
