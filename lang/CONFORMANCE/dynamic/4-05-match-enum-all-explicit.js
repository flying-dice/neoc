function describe(s) {
  return (() => {
    const __s1 = s;
    if (__s1.tag === "Dot") {
      return "dot";
    }
    if (__s1.tag === "Move") {
      const a = __s1._0;
      const b = __s1._1;
      return "move";
    }
    if (__s1.tag === "Circle") {
      const radius = __s1.radius;
      return "circle";
    }
  })();
}
function main() {
  print(describe({ _enum: "Shape", tag: "Dot" }));
  print(describe({ _enum: "Shape", tag: "Move", _0: 1, _1: 2 }));
  print(describe({ _enum: "Shape", tag: "Circle", radius: 5 }));
}
