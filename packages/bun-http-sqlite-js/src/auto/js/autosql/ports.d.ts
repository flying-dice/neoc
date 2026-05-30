import type { Result, Option } from "../neoc-lib";

export type SqlValue = { __enum: "SqlValue"; __tag: "Int"; _0: number } | { __enum: "SqlValue"; __tag: "Real"; _0: number } | { __enum: "SqlValue"; __tag: "Text"; _0: string } | { __enum: "SqlValue"; __tag: "Bool"; _0: boolean } | { __enum: "SqlValue"; __tag: "Null" };
export declare const SqlValue: {
  Int(_0: number): SqlValue;
  Real(_0: number): SqlValue;
  Text(_0: string): SqlValue;
  Bool(_0: boolean): SqlValue;
  Null: SqlValue;
  match<R>(value: SqlValue, arms: { Int(_0: number): R; Real(_0: number): R; Text(_0: string): R; Bool(_0: boolean): R; Null(): R }): R;
};
export type SqlExecutor = {
  execute(sql: string, params: SqlValue[]): Result<number, string>;
  query(sql: string, params: SqlValue[]): Result<SqlValue[][], string>;
};
export declare function bind_opt_int(value: Option<number>): SqlValue;
export declare function bind_opt_real(value: Option<number>): SqlValue;
export declare function bind_opt_text(value: Option<string>): SqlValue;
export declare function bind_opt_bool(value: Option<boolean>): SqlValue;
export declare function decode_number(value: SqlValue, label: string): Result<number, string>;
export declare function decode_opt_number(value: SqlValue, label: string): Result<Option<number>, string>;
export declare function decode_string(value: SqlValue, label: string): Result<string, string>;
export declare function decode_opt_string(value: SqlValue, label: string): Result<Option<string>, string>;
export declare function decode_bool(value: SqlValue, label: string): Result<boolean, string>;
export declare function decode_opt_bool(value: SqlValue, label: string): Result<Option<boolean>, string>;
