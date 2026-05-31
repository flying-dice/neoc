import type { Result, Option } from "./neoc-lib";

import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function describe(o: Option<number>): string {
  return (() => {
    const __s1 = o;
    if (__s1.__tag === "Some") {
      const n = __s1._0;
      return "some";
    }
    return "none";
  })();
}
function main(): void {
  print(describe(Some(3)));
  print(describe(None));
}
