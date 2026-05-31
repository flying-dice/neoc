interface Counter { value: number; clone(): Counter; }

const __t: Record<string, string[]> = {};

const Counter$: Record<string, Function> = {};
Counter$.clone = function(this: Counter) {
  return { __struct: "Counter", value: this.value, __proto__: Counter$ };
};
__t.Counter = ["Clone"];
function main(): void {
  let a = { __struct: "Counter", value: 5, __proto__: Counter$ } as unknown as Counter;
  let b = a.clone();
  println(a.value);
  println(b.value);
}
