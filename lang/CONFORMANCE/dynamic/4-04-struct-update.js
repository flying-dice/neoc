function main() {
  let p = { _struct: "Point", x: 1, y: 2 };
  let q = { ...p, _struct: "Point", x: 9 };
  println(q.x);
  println(q.y);
}
