const __t = {};

const Dog$ = {};
__t.Dog = ["Greet"];
Dog$.greet = function() {
  return (this.name + " says woof");
}
function main() {
  let d = { __struct: "Dog", name: "Rex", __proto__: Dog$ };
  println(d.greet());
}
