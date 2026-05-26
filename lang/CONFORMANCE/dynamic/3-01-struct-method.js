const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv._enum][name](recv, ...args);

__m.Point = {};
__m.Point.manhattan = function(self) {
  return (self.x + self.y);
}
function main() {
  let p = { __struct: "Point", x: 3, y: 4 };
  println(p.x);
  println(__call(p, "manhattan"));
}
