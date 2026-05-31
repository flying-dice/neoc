interface Point { x: number; y: number; display(): string; }

const __t: Record<string, string[]> = {};

const Point$: Record<string, Function> = {};
Point$.display = function(this: Point) {
  return String(this.x) + ", " + String(this.y);
};
__t.Point = ["Display"];
function main(): void {
  let p = { __struct: "Point", x: 1, y: 2, __proto__: Point$ } as unknown as Point;
  println(p.display());
}
