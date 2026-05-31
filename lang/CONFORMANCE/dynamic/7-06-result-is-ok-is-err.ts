import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

function main(): void {
  let ok: Result<number, string> = Ok(1);
  let err: Result<number, string> = Err("bad");
  println(ok.is_ok());
  println(ok.is_err());
  println(err.is_ok());
  println(err.is_err());
}
