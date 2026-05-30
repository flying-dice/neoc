import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let ok = Ok(33);
  println(ok.unwrap());
}
