interface ResultMethods<T, E> {
  is_ok(): boolean;
  is_err(): boolean;
  unwrap(): T;
  unwrap_err(): E;
  map<U>(f: (v: T) => U): Result<U, E>;
  map_err<F>(f: (e: E) => F): Result<T, F>;
}
interface OptionMethods<T> {
  is_some(): boolean;
  is_none(): boolean;
  unwrap(): T;
  unwrap_or(default_: T): T;
  map<U>(f: (v: T) => U): Option<U>;
  ok_or<E>(err: E): Result<T, E>;
}
export type Result<T, E> =
  | ({ __enum: "Result"; __tag: "Ok"; _0: T } & ResultMethods<T, E>)
  | ({ __enum: "Result"; __tag: "Err"; _0: E } & ResultMethods<T, E>);
export type Option<T> =
  | ({ __enum: "Option"; __tag: "Some"; _0: T } & OptionMethods<T>)
  | ({ __enum: "Option"; __tag: "None" } & OptionMethods<T>);
export declare function Ok<T, E>(v: T): Result<T, E>;
export declare function Err<T, E>(e: E): Result<T, E>;
export declare function Some<T>(v: T): Option<T>;
export declare const None: Option<never>;
export declare const Option$: Record<string, Function>;
export declare const Result$: Record<string, Function>;
