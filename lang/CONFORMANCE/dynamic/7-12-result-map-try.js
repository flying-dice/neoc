import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function step(r) {
  const __try_0 = r.map((x) => (x + 10));
  if (__try_0.__tag === "Err" || __try_0.__tag === "None") return __try_0;
  let v = __try_0._0;
  return Ok(v);
}
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
  println(read(step(Ok(5))));
  println(read(step(Err("nope"))));
}
