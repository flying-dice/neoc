const Box$ = {};
Box$.get = function() {
  return this.value;
}
function main() {
  let b = { __struct: "Box", value: 99, __proto__: Box$ };
  println(b.get());
}
