function main() {
  let p = { __struct: "Point", x: 3, y: 4 };
  const __d1 = p;
  const x = __d1.x;
  const y = __d1.y;
  println((x + y));
}
