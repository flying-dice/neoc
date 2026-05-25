const Ok = (v) => ({ _enum: "Result", tag: "Ok", _0: v });
const Err = (e) => ({ _enum: "Result", tag: "Err", _0: e });
const Some = (v) => ({ _enum: "Option", tag: "Some", _0: v });
const None = { _enum: "Option", tag: "None" };

function describe(r) {
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
  print(describe(Ok(7)));
  print(describe(Err("boom")));
}
