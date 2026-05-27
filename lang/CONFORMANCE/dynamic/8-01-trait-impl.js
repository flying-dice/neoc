const __m = {};
const __call = (recv, name, ...args) => __m[recv.__struct ?? recv.__enum][name](recv, ...args);
const __t = {};

__m.Dog = {};
__t.Dog = ["Greet"];
__m.Dog.greet = function(self) {
  return (self.name + " says woof");
}
function main() {
  let d = { __struct: "Dog", name: "Rex" };
  println(__call(d, "greet"));
}
