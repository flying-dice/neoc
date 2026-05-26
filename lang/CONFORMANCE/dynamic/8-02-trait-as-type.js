const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);

__m.Circle = {};
__m.Square = {};
__m.Circle.draw = function(self) {
  return "O";
}
__m.Square.draw = function(self) {
  return "[]";
}
function render(shape) {
  return __call(shape, "draw");
}
function main() {
  println(render({ __struct: "Circle", r: 1 }));
  println(render({ __struct: "Square", side: 2 }));
}
