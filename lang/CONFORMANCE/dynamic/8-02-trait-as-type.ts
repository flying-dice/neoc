type Drawable = {
  draw(): string;
};
interface Circle { r: number; draw(): string; }
interface Square { side: number; draw(): string; }

const __t: Record<string, string[]> = {};

const Circle$: Record<string, Function> = {};
__t.Circle = ["Drawable"];
const Square$: Record<string, Function> = {};
__t.Square = ["Drawable"];
Circle$.draw = function(this: Circle) {
  return "O";
}
Square$.draw = function(this: Square) {
  return "[]";
}
function render(shape: Drawable): string {
  return shape.draw();
}
function main(): void {
  println(render({ __struct: "Circle", r: 1, __proto__: Circle$ } as unknown as Circle));
  println(render({ __struct: "Square", side: 2, __proto__: Square$ } as unknown as Square));
}
