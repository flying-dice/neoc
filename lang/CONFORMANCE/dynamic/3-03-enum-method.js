const __m = {};
const __call = (recv, name, ...args) => __m[recv._struct ?? recv._enum][name](recv, ...args);

__m.Dir = {};
__m.Dir.name = function(self) {
  return (() => {
    const __s1 = self;
    if (__s1.tag === "N") {
      return "north";
    }
    if (__s1.tag === "S") {
      return "south";
    }
    if (__s1.tag === "E") {
      return "east";
    }
    if (__s1.tag === "W") {
      return "west";
    }
  })();
}
function main() {
  let d = { _enum: "Dir", tag: "E" };
  println(__call(d, "name"));
}
