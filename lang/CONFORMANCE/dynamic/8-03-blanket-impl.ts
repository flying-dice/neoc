type Show = {
  show(): string;
};
type Describe = {
  describe(): string;
};
interface Dog { name: string; show(): string; describe(): string; }

const __t: Record<string, string[]> = {};

const Dog$: Record<string, Function> = {};
__t.Dog = ["Describe", "Show"];
Dog$.show = function(this: Dog) {
  return this.name;
}
function main(): void {
  let d = { __struct: "Dog", name: "Rex", __proto__: Dog$ } as unknown as Dog;
  println(d.describe());
}
Dog$.describe = function(this: Dog) {
  return this.show();
}
