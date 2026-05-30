interface Point { x: number; y: number; }

const Point$ = {};
Point$.manhattan = function() {
  return (this.x + this.y);
}
function main(): void {
  let p = { __struct: "Point", x: 3, y: 4, __proto__: Point$ };
  println(p.x);
  println(p.manhattan());
}
