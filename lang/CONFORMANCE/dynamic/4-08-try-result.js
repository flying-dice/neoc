const Ok = (v) => ({ _enum: "Result", tag: "Ok", _0: v });
const Err = (e) => ({ _enum: "Result", tag: "Err", _0: e });
const Some = (v) => ({ _enum: "Option", tag: "Some", _0: v });
const None = { _enum: "Option", tag: "None" };

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
  if (__try_0.tag === "Err" || __try_0.tag === "None") return __try_0;
  let n = __try_0._0;
  return Ok((n + n));
}
function show(r) {
  return (() => {
    const __s1 = r;
    if (__s1.tag === "Ok") {
      const n = __s1._0;
      return "ok";
    }
    if (__s1.tag === "Err") {
      const e = __s1._0;
      return e;
    }
  })();
}
function main() {
  println(show(doubled(true)));
  println(show(doubled(false)));
}
