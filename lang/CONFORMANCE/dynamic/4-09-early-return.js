function clamp(n) {
  if ((n < 0)) {
    return 0;
  }
  if ((n > 100)) {
    return 100;
  }
  return n;
}
function main() {
  println(clamp((-5)));
  println(clamp(250));
  println(clamp(42));
}
