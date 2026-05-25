const __m = {};
const __call = (recv, name, ...args) => __m[recv._struct ?? recv._enum][name](recv, ...args);

__m.Dog = {};
__m.Dog.greet = function(self) {
  return (self.name + " says woof");
}
function main() {
  let d = { _struct: "Dog", name: "Rex" };
  println(__call(d, "greet"));
}
