const Ok = (v) => ({ _enum: "Result", tag: "Ok", _0: v });
const Err = (e) => ({ _enum: "Result", tag: "Err", _0: e });
const Some = (v) => ({ _enum: "Option", tag: "Some", _0: v });
const None = { _enum: "Option", tag: "None" };

function describe(o) {
  return (() => {
    const __s1 = o;
    if (__s1.tag === "Some") {
      const n = __s1._0;
      return "some";
    }
    if (__s1.tag === "None") {
      return "none";
    }
  })();
}
function main() {
  print(describe(Some(3)));
  print(describe(None));
}
