function grade(n: number): string {
  return (() => {
    if ((n >= 90)) {
      return "A";
    } else if ((n >= 80)) {
      return "B";
    } else if ((n >= 70)) {
      return "C";
    } else {
      return "F";
    }
  })();
}
function main(): void {
  println(grade(95));
  println(grade(82));
  println(grade(40));
}
