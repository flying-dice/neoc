import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function dig(o) {
  return (() => {
    const __s1 = o;
    if (__s1.__tag === "Some" && __s1._0.__tag === "Ok") {
      const n = __s1._0._0;
      return n;
    }
    if (__s1.__tag === "Some" && __s1._0.__tag === "Err") {
      return (-1);
    }
    if (__s1.__tag === "None") {
      return 0;
    }
  })();
}
function main() {
  println(dig(Some(Ok(42))));
  println(dig(Some(Err("boom"))));
  println(dig(None));
}
