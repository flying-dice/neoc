const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);

__m.Dir = {};
__m.Dir.name = function(self) {
  return (() => {
    const __s1 = self;
    if (__s1.__tag === "N") {
      return "north";
    }
    if (__s1.__tag === "S") {
      return "south";
    }
    if (__s1.__tag === "E") {
      return "east";
    }
    if (__s1.__tag === "W") {
      return "west";
    }
  })();
}
function main() {
  let d = { __enum: "Dir", __tag: "E" };
  println(__call(d, "name"));
}
