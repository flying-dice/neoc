function classify(n: number): void {
  return (() => {
    const __s1 = n;
    if (__s1 === 0) {
      return println("zero");
    }
    if (__s1 === 1) {
      return println("one");
    }
    return println("many");
  })();
}
function main(): void {
  classify(0);
  classify(1);
  classify(7);
}
