import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

function main() {
  let s = Some(5);
  let n = { __enum: "Option", __tag: "None", __proto__: Option$ };
  println(s.ok_or("x").is_ok());
  println(n.ok_or("x").is_err());
}
