function main(): void {
  let x = (() => {
    let a = 2;
    let b = 3;
    return (a * b);
  })();
  println(x);
}
