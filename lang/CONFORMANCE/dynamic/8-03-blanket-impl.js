const __t = {};

const Dog$ = {};
__t.Dog = ["Describe", "Show"];
Dog$.show = function() {
  return this.name;
}
function main() {
  let d = { __struct: "Dog", name: "Rex", __proto__: Dog$ };
  println(d.describe());
}
Dog$.describe = function() {
  return this.show();
}
