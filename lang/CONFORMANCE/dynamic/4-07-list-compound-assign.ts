function main(): void {
  let xs = [1, 2, 3];
  xs[0]! += 10;
  xs[0]! *= 2;
  xs[1]! *= 5;
  let i = 2;
  xs[i]! -= 1;
  println(xs[0]!);
  println(xs[1]!);
  println(xs[2]!);
}
