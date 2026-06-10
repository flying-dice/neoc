type Tree<T> = { __enum: "Tree"; __tag: "Leaf"; _0: T } | { __enum: "Tree"; __tag: "Empty" };

function value(t: Tree<number>): number {
  return (() => {
    const __s1 = t;
    if (__s1.__tag === "Leaf") {
      const n = __s1._0;
      return n;
    }
    return 0;
  })();
}
function main(): void {
  println(value({ __enum: "Tree", __tag: "Leaf", _0: 5 }));
  println(value({ __enum: "Tree", __tag: "Empty" }));
}
