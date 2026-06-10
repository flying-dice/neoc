interface Point { x: number; y: number; z: number; }

function classify(p: Point): number {
  return (() => {
    const __s1 = p;
    if (__s1.__struct === "Point" && __s1.x === 0) {
      const y = __s1.y;
      return y;
    }
    const x = __s1.x;
    return x;
  })();
}
function main(): void {
  println(classify({ __struct: "Point", x: 0, y: 7, z: 99 }));
  println(classify({ __struct: "Point", x: 5, y: 8, z: 99 }));
}
