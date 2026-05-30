import type { Result, Option } from "./neoc-lib";

import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function describe(r: Result<number, string>): string {
  return (() => {
    const __s1 = r;
    if (__s1.__tag === "Ok") {
      const n = __s1._0;
      return "ok";
    }
    if (__s1.__tag === "Err") {
      const e = __s1._0;
      return e;
    }
  })();
}
function main(): void {
  print(describe(Ok(7)));
  print(describe(Err("boom")));
}
