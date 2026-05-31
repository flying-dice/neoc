type Shape = { __enum: "Shape"; __tag: "Dot" } | { __enum: "Shape"; __tag: "Move"; _0: number; _1: number } | { __enum: "Shape"; __tag: "Box"; w: number; h: number };

function area(s: Shape): number {
  return (() => {
    const __s1 = s;
    if (__s1.__tag === "Move") {
      const dx = __s1._0;
      const dy = __s1._1;
      return (dx + dy);
    }
    if (__s1.__tag === "Box") {
      const w = __s1.w;
      const h = __s1.h;
      return (w * h);
    }
    return 0;
  })();
}
function main(): void {
  println(area({ __enum: "Shape", __tag: "Move", _0: 3, _1: 4 }));
  println(area({ __enum: "Shape", __tag: "Box", w: 5, h: 6 }));
  println(area({ __enum: "Shape", __tag: "Dot" }));
}
