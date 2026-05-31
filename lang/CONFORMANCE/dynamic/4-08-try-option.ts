import type { Result, Option } from "./neoc-lib";

import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function incr(o: Option<number>): Option<number> {
  const __try_0 = o;
  if (__try_0.__tag !== "Some") return __try_0 as unknown as Option<number>;
  let n = __try_0._0;
  return Some((n + 1));
}
function unwrap_or(o: Option<number>): number {
  return (() => {
    const __s1 = o;
    if (__s1.__tag === "Some") {
      const n = __s1._0;
      return n;
    }
    return 0;
  })();
}
function main(): void {
  println(unwrap_or(incr(Some(41))));
  println(unwrap_or(incr(None)));
}
