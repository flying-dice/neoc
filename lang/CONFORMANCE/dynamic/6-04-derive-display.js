const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);
const __t = {};

__m.Point = {};
__m.Point.display = function(self) {
  return String(self.x) + ", " + String(self.y);
};
__t.Point = ["Display"];
function main() {
  let p = { __struct: "Point", x: 1, y: 2 };
  println(__call(p, "display"));
}
