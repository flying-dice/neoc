function adder(base) {
  return (x) => (x + base);
}
function main() {
  let add10 = adder(10);
  println(add10(5));
  println(add10(32));
}
