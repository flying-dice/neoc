const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);

__m.Counter = {};
__m.Counter.start = function(v) {
  return { __struct: "Counter", value: v };
}
__m.Counter.bump = function(self) {
  return (self.value + 1);
}
function main() {
  let c = __m.Counter.start(41);
  println(c.value);
  println(__call(c, "bump"));
}
