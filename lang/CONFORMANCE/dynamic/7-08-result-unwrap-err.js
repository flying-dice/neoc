import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function main() {
  let err = Err("boom");
  println(err.unwrap_err());
}
