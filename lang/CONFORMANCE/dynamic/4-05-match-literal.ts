function classify(n: number): number {
  return (() => {
    const __s1 = n;
    if (__s1 === 0) {
      return 100;
    }
    if (__s1 === 1) {
      return 200;
    }
    return 999;
  })();
}
function main(): void {
  println(classify(0));
  println(classify(1));
  println(classify(7));
}
