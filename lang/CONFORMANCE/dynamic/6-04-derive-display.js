const __m = {};
const __call = (recv, name, ...args) => __m[recv._struct ?? recv._enum][name](recv, ...args);

__m.Point = {};
__m.Point.display = function(self) {
  return String(self.x) + ", " + String(self.y);
};
function main() {
  let p = { _struct: "Point", x: 1, y: 2 };
  println(__call(p, "display"));
}
