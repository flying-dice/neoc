function describe(s) {
  return (() => {
    const __s1 = s;
    if (__s1.__tag === "Dot") {
      return "dot";
    }
    if (__s1.__tag === "Move") {
      const a = __s1._0;
      const b = __s1._1;
      return "move";
    }
    if (__s1.__tag === "Circle") {
      const radius = __s1.radius;
      return "circle";
    }
  })();
}
function main() {
  print(describe({ __enum: "Shape", __tag: "Dot" }));
  print(describe({ __enum: "Shape", __tag: "Move", _0: 1, _1: 2 }));
  print(describe({ __enum: "Shape", __tag: "Circle", radius: 5 }));
}
