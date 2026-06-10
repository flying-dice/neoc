type Signal = ({ __enum: "Signal"; __tag: "Pulse"; _0: number } | { __enum: "Signal"; __tag: "Idle" }) & { clone(): Signal; eq(a: Signal, b: Signal): boolean; debug(): string; display(): string; };

const __t: Record<string, string[]> = {};

const Signal$: Record<string, Function> = {};
Signal$.clone = function(this: Signal) {
  if (this.__tag === "Pulse") {
    return { __enum: "Signal", __tag: "Pulse", _0: this._0, __proto__: Signal$ };
  }
  if (this.__tag === "Idle") {
    return { __enum: "Signal", __tag: "Idle", __proto__: Signal$ };
  }
};
Signal$.eq = function(a, b) {
  if (a.__enum !== b.__enum || a.__tag !== b.__tag) {
    return false;
  }
  if (a.__tag === "Pulse") {
    return a._0 === b._0;
  }
  return true;
};
Signal$.debug = function(this: Signal) {
  if (this.__tag === "Pulse") {
    return "Pulse(" + String(this._0) + ")";
  }
  if (this.__tag === "Idle") {
    return "Idle";
  }
};
Signal$.display = function(this: Signal) {
  if (this.__tag === "Pulse") {
    return String(this._0);
  }
  if (this.__tag === "Idle") {
    return "Idle";
  }
};
__t.Signal = ["Clone", "Debug", "Display", "Eq"];
function main(): void {
  let a = { __enum: "Signal", __tag: "Pulse", _0: 7, __proto__: Signal$ } as unknown as Signal;
  let b = a.clone();
  if (a.eq(a, b)) {
    println("clone-eq");
  }
  let c = { __enum: "Signal", __tag: "Idle", __proto__: Signal$ } as unknown as Signal;
  if ((!a.eq(a, c))) {
    println("tag-ne");
  }
  let d = { __enum: "Signal", __tag: "Pulse", _0: 9, __proto__: Signal$ } as unknown as Signal;
  if ((!a.eq(a, d))) {
    println("payload-ne");
  }
  println(a.debug());
  println(c.debug());
  println(a.display());
  println(c.display());
}
