function area(s) {
  return (() => {
    const __s1 = s;
    if (__s1.tag === "Move") {
      const dx = __s1._0;
      const dy = __s1._1;
      return (dx + dy);
    }
    if (__s1.tag === "Box") {
      const w = __s1.w;
      const h = __s1.h;
      return (w * h);
    }
    if (true) {
      return 0;
    }
  })();
}
function main() {
  println(area({ _enum: "Shape", tag: "Move", _0: 3, _1: 4 }));
  println(area({ _enum: "Shape", tag: "Box", w: 5, h: 6 }));
  println(area({ _enum: "Shape", tag: "Dot" }));
}
