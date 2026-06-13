type Container = {
  Item: unknown;
  peek(): unknown;
};
interface Bin { value: number; peek(): number; }

const __t: Record<string, string[]> = {};

const Bin$: Record<string, Function> = {};
__t.Bin = ["Container"];
Bin$.peek = function(this: Bin) {
  return this.value;
}
function main(): void {
  let b = { __struct: "Bin", value: 7, __proto__: Bin$ } as unknown as Bin;
  println(b.peek());
}
