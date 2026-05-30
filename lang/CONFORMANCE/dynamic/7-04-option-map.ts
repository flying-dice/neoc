import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let s = Some(20);
  let n = { __enum: "Option", __tag: "None", __proto__: Option$ };
  println(s.map((x) => (x + 1)).unwrap_or(0));
  println(n.map((x) => (x + 1)).unwrap_or(0));
}
