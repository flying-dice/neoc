interface Counter { value: number; bump(): number; }

const Counter$: Record<string, Function> = {};
Counter$.start = function(v: number) {
  return { __struct: "Counter", value: v, __proto__: Counter$ } as unknown as Counter;
}
Counter$.bump = function(this: Counter) {
  return (this.value + 1);
}
function main(): void {
  let c = Counter$.start(41);
  println(c.value);
  println(c.bump());
}
