import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function main() {
  let ok = Ok(33);
  println(ok.unwrap());
}
