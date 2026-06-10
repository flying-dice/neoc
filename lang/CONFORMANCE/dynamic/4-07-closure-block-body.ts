function main(): void {
  let stepped = (n) => (() => {
    let doubled = (n * 2);
    return (doubled + 1);
  })();
  println(stepped(20));
  println(stepped(0));
}
