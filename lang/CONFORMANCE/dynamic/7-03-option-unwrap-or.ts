import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let s = Some(7);
  let n = { __enum: "Option", __tag: "None", __proto__: Option$ };
  println(s.unwrap_or(0));
  println(n.unwrap_or(99));
}
