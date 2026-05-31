import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let err: Result<number, string> = Err("boom");
  println(err.unwrap_err());
}
