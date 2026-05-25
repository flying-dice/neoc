const Ok = (v) => ({ _enum: "Result", tag: "Ok", _0: v });
const Err = (e) => ({ _enum: "Result", tag: "Err", _0: e });
const Some = (v) => ({ _enum: "Option", tag: "Some", _0: v });
const None = { _enum: "Option", tag: "None" };

function incr(o) {
  const __try_0 = o;
  if (__try_0.tag === "Err" || __try_0.tag === "None") return __try_0;
  let n = __try_0._0;
  return Some((n + 1));
}
function unwrap_or(o) {
  return (() => {
    const __s1 = o;
    if (__s1.tag === "Some") {
      const n = __s1._0;
      return n;
    }
    if (__s1.tag === "None") {
      return 0;
    }
  })();
}
function main() {
  println(unwrap_or(incr(Some(41))));
  println(unwrap_or(incr(None)));
}
