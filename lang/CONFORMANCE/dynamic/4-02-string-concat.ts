function greet(name: string): string {
  return (("hello, " + name) + "!");
}
function main(): void {
  println(greet("world"));
}
