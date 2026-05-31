type Dir = ({ __enum: "Dir"; __tag: "N" } | { __enum: "Dir"; __tag: "S" } | { __enum: "Dir"; __tag: "E" } | { __enum: "Dir"; __tag: "W" }) & { name(): string; };

const Dir$: Record<string, Function> = {};
Dir$.name = function(this: Dir) {
  return (() => {
    const __s1 = this;
    if (__s1.__tag === "N") {
      return "north";
    }
    if (__s1.__tag === "S") {
      return "south";
    }
    if (__s1.__tag === "E") {
      return "east";
    }
    return "west";
  })();
}
function main(): void {
  let d = { __enum: "Dir", __tag: "E", __proto__: Dir$ } as unknown as Dir;
  println(d.name());
}
