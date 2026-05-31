import type { Result, Option } from "./neoc-lib";

import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function step(r: Result<number, string>): Result<number, string> {
  const __try_0 = r.map((x) => (x + 10));
  if (__try_0.__tag !== "Ok") return __try_0 as unknown as Result<number, string>;
  let v = __try_0._0;
  return Ok(v);
}
function read(r: Result<number, string>): number {
  return (() => {
    const __s1 = r;
    if (__s1.__tag === "Ok") {
      const v = __s1._0;
      return v;
    }
    return 0;
  })();
}
function main(): void {
  println(read(step(Ok(5))));
  println(read(step(Err("nope"))));
}
