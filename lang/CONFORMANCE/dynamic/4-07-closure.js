function apply(f, x) {
  return f(x);
}
function main() {
  let double = (n) => (n * 2);
  println(apply(double, 21));
}
