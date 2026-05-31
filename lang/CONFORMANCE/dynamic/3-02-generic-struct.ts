interface Box<T> { value: T; get(): T; }

const Box$: Record<string, Function> = {};
Box$.get = function(this: Box) {
  return this.value;
}
function main(): void {
  let b = { __struct: "Box", value: 99, __proto__: Box$ } as unknown as Box;
  println(b.get());
}
