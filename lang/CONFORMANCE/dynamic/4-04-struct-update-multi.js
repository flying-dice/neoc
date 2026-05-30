function main() {
  let v = { __struct: "Vec3", x: 1, y: 2, z: 3 };
  let w = { ...v, __struct: "Vec3", x: 10, z: 30 };
  println(w.x);
  println(w.y);
  println(w.z);
}
