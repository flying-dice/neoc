interface Point { x: number; y: number; manhattan(): number; }

const Point$: Record<string, Function> = {};
Point$.manhattan = function(this: Point) {
  return (this.x + this.y);
}
function main(): void {
  let p = { __struct: "Point", x: 3, y: 4, __proto__: Point$ } as unknown as Point;
  println(p.x);
  println(p.manhattan());
}
