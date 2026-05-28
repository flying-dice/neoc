const __t = {};

const Point$ = {};
Point$.debug = function() {
  return "Point { " + "x = " + String(this.x) + ", " + "y = " + String(this.y) + " }";
};
__t.Point = ["Debug"];
function main() {
  let p = { __struct: "Point", x: 1, y: 2, __proto__: Point$ };
  println(p.debug());
}
