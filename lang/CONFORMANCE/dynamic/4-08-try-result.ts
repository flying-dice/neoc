import type { Result, Option } from "./neoc-lib";

import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function parse(ok: boolean): Result<number, string> {
  return (() => {
    if (ok) {
      return Ok(10);
    } else {
      return Err("bad");
    }
  })();
}
function doubled(ok: boolean): Result<number, string> {
  const __try_0 = parse(ok);
  if (__try_0.__tag !== "Ok") return __try_0 as unknown as Result<number, string>;
  let n = __try_0._0;
  return Ok((n + n));
}
function show(r: Result<number, string>): string {
  return (() => {
    const __s1 = r;
    if (__s1.__tag === "Ok") {
      const n = __s1._0;
      return "ok";
    }
    const e = __s1._0;
    return e;
  })();
}
function main(): void {
  println(show(doubled(true)));
  println(show(doubled(false)));
}
