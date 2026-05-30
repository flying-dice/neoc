import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function main() {
  let s = Some(42);
  println(s.unwrap());
}
