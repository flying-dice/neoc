type Show = {
  show(): string;
};
type Describe = {
  describe(): string;
};
interface Dog { name: string; }

const __t = {};

const Dog$ = {};
__t.Dog = ["Describe", "Show"];
Dog$.show = function() {
  return this.name;
}
function main(): void {
  let d = { __struct: "Dog", name: "Rex", __proto__: Dog$ };
  println(d.describe());
}
Dog$.describe = function() {
  return this.show();
}
