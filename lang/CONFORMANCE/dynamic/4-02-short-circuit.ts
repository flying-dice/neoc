function loud(tag: string): boolean {
  println(tag);
  return true;
}
function main(): void {
  (false && loud("and-skipped"));
  (true || loud("or-skipped"));
  (true && loud("and-run"));
  (false || loud("or-run"));
}
