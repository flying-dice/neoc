import type { Result, Option } from "./neoc-lib";

import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function read(r: Result<number, string>): number {
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
function main(): void {
  let ok = Ok(10);
  let err = Err("e");
  println(read(ok.map((x) => (x + 5))));
  println(read(err.map((x) => (x + 5))));
}
