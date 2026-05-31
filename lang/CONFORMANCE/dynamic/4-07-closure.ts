function apply(f: (a0: number) => number, x: number): number {
  return f(x);
}
function main(): void {
  let double = (n) => (n * 2);
  println(apply(double, 21));
}
