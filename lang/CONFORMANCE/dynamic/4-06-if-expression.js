function main() {
  let x = 7;
  let y = (() => {
    if ((x > 5)) {
      return (x * 2);
    } else {
      return 0;
    }
  })();
  y = (y + 1);
  println(y);
}
