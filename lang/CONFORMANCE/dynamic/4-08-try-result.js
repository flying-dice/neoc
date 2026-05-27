const Ok = (v) => ({ __enum: "Result", __tag: "Ok", _0: v });
const Err = (e) => ({ __enum: "Result", __tag: "Err", _0: e });
const Some = (v) => ({ __enum: "Option", __tag: "Some", _0: v });
const None = { __enum: "Option", __tag: "None" };

function parse(ok) {
  return (() => {
    if (ok) {
      return Ok(10);
    } else {
      return Err("bad");
    }
  })();
}
function doubled(ok) {
  const __try_0 = parse(ok);
  if (__try_0.__tag === "Err" || __try_0.__tag === "None") return __try_0;
  let n = __try_0._0;
  return Ok((n + n));
}
function show(r) {
  return (() => {
    const __s1 = r;
    if (__s1.__tag === "Ok") {
      const n = __s1._0;
      return "ok";
    }
    if (__s1.__tag === "Err") {
      const e = __s1._0;
      return e;
    }
  })();
}
function main() {
  println(show(doubled(true)));
  println(show(doubled(false)));
}
