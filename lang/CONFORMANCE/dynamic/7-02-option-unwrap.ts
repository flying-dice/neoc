import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let s = Some(42);
  println(s.unwrap());
}
