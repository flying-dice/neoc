import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function read_err(r) {
  return (() => {
    const __s1 = r;
    if (__s1.__tag === "Ok") {
      return "ok";
    }
    if (__s1.__tag === "Err") {
      const e = __s1._0;
      return e;
    }
  })();
}
function main() {
  let ok = Ok(1);
  let err = Err("x");
  println(read_err(ok.map_err((e) => e)));
  println(read_err(err.map_err((e) => e)));
}
