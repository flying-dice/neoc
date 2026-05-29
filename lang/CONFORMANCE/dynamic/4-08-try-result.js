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

function parse(ok) {
  return (() => {
    if (ok) {
      return Ok(10);
    } else {
      return Err("bad");
    }
  })();
}
function doubled(ok) {
  const __try_0 = parse(ok);
  if (__try_0.__tag === "Err" || __try_0.__tag === "None") return __try_0;
  let n = __try_0._0;
  return Ok((n + n));
}
function show(r) {
  return (() => {
    const __s1 = r;
    if (__s1.__tag === "Ok") {
      const n = __s1._0;
      return "ok";
    }
    if (__s1.__tag === "Err") {
      const e = __s1._0;
      return e;
    }
  })();
}
function main() {
  println(show(doubled(true)));
  println(show(doubled(false)));
}
