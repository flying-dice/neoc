import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function describe(o) {
  return (() => {
    const __s1 = o;
    if (__s1.__tag === "Some") {
      const n = __s1._0;
      return "some";
    }
    if (__s1.__tag === "None") {
      return "none";
    }
  })();
}
function main() {
  print(describe(Some(3)));
  print(describe(None));
}
