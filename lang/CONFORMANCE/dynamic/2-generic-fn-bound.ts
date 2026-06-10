type Doubler = {
  double(): number;
};
interface Num { v: number; double(): number; }

const __t: Record<string, string[]> = {};

const Num$: Record<string, Function> = {};
__t.Num = ["Doubler"];
Num$.double = function(this: Num) {
  return (this.v + this.v);
}
function twice<T extends Doubler>(x: T): number {
  return x.double();
}
function main(): void {
  println(twice({ __struct: "Num", v: 21, __proto__: Num$ } as unknown as Num));
}
