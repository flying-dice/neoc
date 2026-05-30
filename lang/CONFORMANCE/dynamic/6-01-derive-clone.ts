interface Counter { value: number; }

const __t = {};

const Counter$ = {};
Counter$.clone = function() {
  return { __struct: "Counter", value: this.value, __proto__: Counter$ };
};
__t.Counter = ["Clone"];
function main(): void {
  let a = { __struct: "Counter", value: 5, __proto__: Counter$ };
  let b = a.clone();
  println(a.value);
  println(b.value);
}
