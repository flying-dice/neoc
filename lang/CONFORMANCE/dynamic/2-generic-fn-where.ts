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
function combine<A extends Doubler, B extends Doubler>(a: A, b: B): number {
  return (a.double() + b.double());
}
function main(): void {
  println(combine({ __struct: "Num", v: 21, __proto__: Num$ } as unknown as Num, { __struct: "Num", v: 5, __proto__: Num$ } as unknown as Num));
}
