const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);
const __t = {};

__m.Dog = {};
__t.Dog = ["Describe", "Show"];
__m.Dog.show = function(self) {
  return self.name;
}
function main() {
  let d = { __struct: "Dog", name: "Rex" };
  println(__call(d, "describe"));
}
__m.Dog.describe = function(self) {
  return __call(self, "show");
}
