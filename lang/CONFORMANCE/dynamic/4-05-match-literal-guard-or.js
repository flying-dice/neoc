function classify(n) {
  return (() => {
    const __s1 = n;
    if (__s1 === 0) {
      return "zero";
    }
    if (((__s1 === 1) || (__s1 === 2) || (__s1 === 3))) {
      return "small";
    }
    if (true) {
      const x = __s1;
      if ((x > 100)) {
        return "huge";
      }
    }
    if (true) {
      return "other";
    }
  })();
}
function main() {
  println(classify(0));
  println(classify(2));
  println(classify(200));
  println(classify(50));
}
