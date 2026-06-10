function main(): void {
  let xs = [1, 2, 3];
  xs[0] = 10;
  xs[1] = 20;
  let i = 2;
  xs[i] = 30;
  println(xs[0]!);
  println(xs[1]!);
  println(xs[2]!);
}
