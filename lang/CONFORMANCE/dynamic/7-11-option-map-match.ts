import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let s = Some(4);
  return (() => {
    const __s1 = s.map((x) => (x * 2));
    if (__s1.__tag === "Some") {
      const v = __s1._0;
      return println(v);
    }
    return println(0);
  })();
}
