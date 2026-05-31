interface Point { x: number; y: number; debug(): string; }

const __t: Record<string, string[]> = {};

const Point$: Record<string, Function> = {};
Point$.debug = function(this: Point) {
  return "Point { " + "x = " + String(this.x) + ", " + "y = " + String(this.y) + " }";
};
__t.Point = ["Debug"];
function main(): void {
  let p = { __struct: "Point", x: 1, y: 2, __proto__: Point$ } as unknown as Point;
  println(p.debug());
}
