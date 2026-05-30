function loud(tag) {
  println(tag);
  return true;
}
function main() {
  (false && loud("and-skipped"));
  (true || loud("or-skipped"));
  (true && loud("and-run"));
  (false || loud("or-run"));
}
