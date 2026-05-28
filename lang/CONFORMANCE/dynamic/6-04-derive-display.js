const __t = {};

const Point$ = {};
Point$.display = function() {
  return String(this.x) + ", " + String(this.y);
};
__t.Point = ["Display"];
function main() {
  let p = { __struct: "Point", x: 1, y: 2, __proto__: Point$ };
  println(p.display());
}
