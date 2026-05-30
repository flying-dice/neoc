import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let s = Some(1);
  let n = { __enum: "Option", __tag: "None", __proto__: Option$ };
  println(s.is_some());
  println(s.is_none());
  println(n.is_some());
  println(n.is_none());
}
