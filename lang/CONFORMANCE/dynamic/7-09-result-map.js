import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function read(r) {
  return (() => {
    const __s1 = r;
    if (__s1.__tag === "Ok") {
      const v = __s1._0;
      return v;
    }
    if (__s1.__tag === "Err") {
      return 0;
    }
  })();
}
function main() {
  let ok = Ok(10);
  let err = Err("e");
  println(read(ok.map((x) => (x + 5))));
  println(read(err.map((x) => (x + 5))));
}
