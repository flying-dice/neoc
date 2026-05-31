function adder(base: number): (a0: number) => number {
  return (x) => (x + base);
}
function main(): void {
  let add10 = adder(10);
  println(add10(5));
  println(add10(32));
}
