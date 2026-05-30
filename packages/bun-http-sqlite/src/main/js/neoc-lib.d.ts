export type Result<T, E> =
  | { __enum: "Result"; __tag: "Ok"; _0: T }
  | { __enum: "Result"; __tag: "Err"; _0: E };
export type Option<T> =
  | { __enum: "Option"; __tag: "Some"; _0: T }
  | { __enum: "Option"; __tag: "None" };
export declare function Ok<T, E>(v: T): Result<T, E>;
export declare function Err<T, E>(e: E): Result<T, E>;
export declare function Some<T>(v: T): Option<T>;
export declare const None: Option<never>;
export declare const Option$: Record<string, Function>;
export declare const Result$: Record<string, Function>;
