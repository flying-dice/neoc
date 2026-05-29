const Option$ = {};
const Result$ = {};
const Ok = (v) => ({ __enum: "Result", __tag: "Ok", _0: v, __proto__: Result$ });
const Err = (e) => ({ __enum: "Result", __tag: "Err", _0: e, __proto__: Result$ });
const Some = (v) => ({ __enum: "Option", __tag: "Some", _0: v, __proto__: Option$ });
const None = { __enum: "Option", __tag: "None", __proto__: Option$ };
Option$.is_some = function() { return this.__tag === "Some"; };
Option$.is_none = function() { return this.__tag === "None"; };
Option$.unwrap = function() {
  if (this.__tag === "Some") return this._0;
  throw new Error("called unwrap on a None value");
};
Option$.unwrap_or = function(default_) { return this.__tag === "Some" ? this._0 : default_; };
Option$.map = function(f) { return this.__tag === "Some" ? Some(f(this._0)) : this; };
Option$.ok_or = function(err) { return this.__tag === "Some" ? Ok(this._0) : Err(err); };
Result$.is_ok = function() { return this.__tag === "Ok"; };
Result$.is_err = function() { return this.__tag === "Err"; };
Result$.unwrap = function() {
  if (this.__tag === "Ok") return this._0;
  throw new Error("called unwrap on an Err value");
};
Result$.unwrap_err = function() {
  if (this.__tag === "Err") return this._0;
  throw new Error("called unwrap_err on an Ok value");
};
Result$.map = function(f) { return this.__tag === "Ok" ? Ok(f(this._0)) : this; };
Result$.map_err = function(f) { return this.__tag === "Err" ? Err(f(this._0)) : this; };

function main() {
  let s = Some(20);
  let n = { __enum: "Option", __tag: "None", __proto__: Option$ };
  println(s.map((x) => (x + 1)).unwrap_or(0));
  println(n.map((x) => (x + 1)).unwrap_or(0));
}
