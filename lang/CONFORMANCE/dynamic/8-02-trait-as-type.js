const __t = {};

const Circle$ = {};
__t.Circle = ["Drawable"];
const Square$ = {};
__t.Square = ["Drawable"];
Circle$.draw = function() {
  return "O";
}
Square$.draw = function() {
  return "[]";
}
function render(shape) {
  return shape.draw();
}
function main() {
  println(render({ __struct: "Circle", r: 1, __proto__: Circle$ }));
  println(render({ __struct: "Square", side: 2, __proto__: Square$ }));
}
