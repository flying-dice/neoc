interface Box<T> { value: T; }

const Box$ = {};
Box$.get = function() {
  return this.value;
}
function main(): void {
  let b = { __struct: "Box", value: 99, __proto__: Box$ };
  println(b.get());
}
