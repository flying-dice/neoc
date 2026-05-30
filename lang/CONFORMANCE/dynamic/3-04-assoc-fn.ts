interface Counter { value: number; }

const Counter$ = {};
Counter$.start = function(v: number) {
  return { __struct: "Counter", value: v, __proto__: Counter$ };
}
Counter$.bump = function() {
  return (this.value + 1);
}
function main(): void {
  let c = Counter$.start(41);
  println(c.value);
  println(c.bump());
}
