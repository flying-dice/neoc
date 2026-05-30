function yes_no(b: boolean): string {
  return (() => {
    const __s1 = b;
    if (__s1 === true) {
      return "yes";
    }
    if (__s1 === false) {
      return "no";
    }
  })();
}
function main(): void {
  println(yes_no(true));
  println(yes_no(false));
}
