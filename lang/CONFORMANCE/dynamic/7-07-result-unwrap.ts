import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let ok: Result<number, string> = Ok(33);
  println(ok.unwrap());
}
