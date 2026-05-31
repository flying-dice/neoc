type Greet = {
  greet(): string;
};
interface Dog { name: string; greet(): string; }

const __t: Record<string, string[]> = {};

const Dog$: Record<string, Function> = {};
__t.Dog = ["Greet"];
Dog$.greet = function(this: Dog) {
  return (this.name + " says woof");
}
function main(): void {
  let d = { __struct: "Dog", name: "Rex", __proto__: Dog$ } as unknown as Dog;
  println(d.greet());
}
