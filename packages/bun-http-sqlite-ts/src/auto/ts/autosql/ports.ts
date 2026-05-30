import type { Result, Option } from "../neoc-lib";

export type SqlValue = { __enum: "SqlValue"; __tag: "Int"; _0: number } | { __enum: "SqlValue"; __tag: "Real"; _0: number } | { __enum: "SqlValue"; __tag: "Text"; _0: string } | { __enum: "SqlValue"; __tag: "Bool"; _0: boolean } | { __enum: "SqlValue"; __tag: "Null" };
export type SqlExecutor = {
  execute(sql: string, params: SqlValue[]): Result<number, string>;
  query(sql: string, params: SqlValue[]): Result<SqlValue[][], string>;
};

import { Ok, Err, Some, None, Option$, Result$ } from "../neoc-lib";

export const SqlValue = {
  Int: (_0: number): SqlValue => ({ __enum: "SqlValue", __tag: "Int", _0 }),
  Real: (_0: number): SqlValue => ({ __enum: "SqlValue", __tag: "Real", _0 }),
  Text: (_0: string): SqlValue => ({ __enum: "SqlValue", __tag: "Text", _0 }),
  Bool: (_0: boolean): SqlValue => ({ __enum: "SqlValue", __tag: "Bool", _0 }),
  Null: { __enum: "SqlValue", __tag: "Null" } as SqlValue,
  match: <R>(value: SqlValue, arms: { Int(_0: number): R; Real(_0: number): R; Text(_0: string): R; Bool(_0: boolean): R; Null(): R }): R => value.__tag === "Int" ? arms.Int(value._0) : value.__tag === "Real" ? arms.Real(value._0) : value.__tag === "Text" ? arms.Text(value._0) : value.__tag === "Bool" ? arms.Bool(value._0) : arms.Null(),
};
export function bind_opt_int(value: Option<number>): SqlValue {
  return (() => {
    const __s1 = value;
    if (__s1.__tag === "Some") {
      const inner = __s1._0;
      return { __enum: "SqlValue", __tag: "Int", _0: inner };
    }
    if (__s1.__tag === "None") {
      return { __enum: "SqlValue", __tag: "Null" };
    }
  })();
}
export function bind_opt_real(value: Option<number>): SqlValue {
  return (() => {
    const __s2 = value;
    if (__s2.__tag === "Some") {
      const inner = __s2._0;
      return { __enum: "SqlValue", __tag: "Real", _0: inner };
    }
    if (__s2.__tag === "None") {
      return { __enum: "SqlValue", __tag: "Null" };
    }
  })();
}
export function bind_opt_text(value: Option<string>): SqlValue {
  return (() => {
    const __s3 = value;
    if (__s3.__tag === "Some") {
      const inner = __s3._0;
      return { __enum: "SqlValue", __tag: "Text", _0: inner };
    }
    if (__s3.__tag === "None") {
      return { __enum: "SqlValue", __tag: "Null" };
    }
  })();
}
export function bind_opt_bool(value: Option<boolean>): SqlValue {
  return (() => {
    const __s4 = value;
    if (__s4.__tag === "Some") {
      const inner = __s4._0;
      return { __enum: "SqlValue", __tag: "Bool", _0: inner };
    }
    if (__s4.__tag === "None") {
      return { __enum: "SqlValue", __tag: "Null" };
    }
  })();
}
export function decode_number(value: SqlValue, label: string): Result<number, string> {
  return (() => {
    const __s5 = value;
    if (((__s5.__tag === "Int") || (__s5.__tag === "Real"))) {
      const inner = __s5._0;
      return Ok(inner);
    }
    if (true) {
      return Err(label);
    }
  })();
}
export function decode_opt_number(value: SqlValue, label: string): Result<Option<number>, string> {
  return (() => {
    const __s6 = value;
    if (__s6.__tag === "Null") {
      return Ok(None);
    }
    if (((__s6.__tag === "Int") || (__s6.__tag === "Real"))) {
      const inner = __s6._0;
      return Ok(Some(inner));
    }
    if (true) {
      return Err(label);
    }
  })();
}
export function decode_string(value: SqlValue, label: string): Result<string, string> {
  return (() => {
    const __s7 = value;
    if (__s7.__tag === "Text") {
      const inner = __s7._0;
      return Ok(inner);
    }
    if (true) {
      return Err(label);
    }
  })();
}
export function decode_opt_string(value: SqlValue, label: string): Result<Option<string>, string> {
  return (() => {
    const __s8 = value;
    if (__s8.__tag === "Null") {
      return Ok(None);
    }
    if (__s8.__tag === "Text") {
      const inner = __s8._0;
      return Ok(Some(inner));
    }
    if (true) {
      return Err(label);
    }
  })();
}
export function decode_bool(value: SqlValue, label: string): Result<boolean, string> {
  return (() => {
    const __s9 = value;
    if (__s9.__tag === "Bool") {
      const inner = __s9._0;
      return Ok(inner);
    }
    if (true) {
      return Err(label);
    }
  })();
}
export function decode_opt_bool(value: SqlValue, label: string): Result<Option<boolean>, string> {
  return (() => {
    const __s10 = value;
    if (__s10.__tag === "Null") {
      return Ok(None);
    }
    if (__s10.__tag === "Bool") {
      const inner = __s10._0;
      return Ok(Some(inner));
    }
    if (true) {
      return Err(label);
    }
  })();
}
