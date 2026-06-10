function main(): void {
  let xs = [2, 4, 6, 8];
  let sum = 0;
  for (const x of xs) {
    sum = (sum + x);
  }
  println(sum);
  let acc = 0;
  for (const x of xs) {
    acc = ((acc * 10) + x);
  }
  println(acc);
}
