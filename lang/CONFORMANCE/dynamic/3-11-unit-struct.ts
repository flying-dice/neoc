interface Sun {shine(): string; }
interface Moon {shine(): string; }

const Sun$: Record<string, Function> = {};
const Sun = { __struct: "Sun", __proto__: Sun$ } as unknown as Sun;
const Moon$: Record<string, Function> = {};
const Moon = { __struct: "Moon", __proto__: Moon$ } as unknown as Moon;
Sun$.shine = function(this: Sun) {
  return "day";
}
Moon$.shine = function(this: Moon) {
  return "night";
}
function main(): void {
  let a = Sun;
  let b = Moon;
  println(a.shine());
  println(b.shine());
}
