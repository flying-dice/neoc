function clamp(n: number): number {
  if ((n < 0)) {
    return 0;
  }
  if ((n > 100)) {
    return 100;
  }
  return n;
}
function main(): void {
  println(clamp((-5)));
  println(clamp(250));
  println(clamp(42));
}
