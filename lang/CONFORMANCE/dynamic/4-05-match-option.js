const Ok = (v) => ({ __enum: "Result", __tag: "Ok", _0: v });
const Err = (e) => ({ __enum: "Result", __tag: "Err", _0: e });
const Some = (v) => ({ __enum: "Option", __tag: "Some", _0: v });
const None = { __enum: "Option", __tag: "None" };

function describe(o) {
  return (() => {
    const __s1 = o;
    if (__s1.__tag === "Some") {
      const n = __s1._0;
      return "some";
    }
    if (__s1.__tag === "None") {
      return "none";
    }
  })();
}
function main() {
  print(describe(Some(3)));
  print(describe(None));
}
