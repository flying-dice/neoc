function main() {
  let p = { __struct: "Point", x: 1, y: 2 };
  let q = { ...p, __struct: "Point", x: 9 };
  println(q.x);
  println(q.y);
}
