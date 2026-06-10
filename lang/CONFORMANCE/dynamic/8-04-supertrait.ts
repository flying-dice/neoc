type Show = {
  show(): string;
};
type Greet = {
  greet(): string;
};
interface Badger { name: string; show(): string; greet(): string; }

const __t: Record<string, string[]> = {};

const Badger$: Record<string, Function> = {};
__t.Badger = ["Greet", "Show"];
Badger$.show = function(this: Badger) {
  return this.name;
}
Badger$.greet = function(this: Badger) {
  return ("Hi, " + this.show());
}
function main(): void {
  let b = { __struct: "Badger", name: "Bram", __proto__: Badger$ } as unknown as Badger;
  println(b.greet());
}
