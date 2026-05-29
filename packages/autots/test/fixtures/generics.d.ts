// Generic surfaces: type parameters on an interface, a class, a free function,
// and a method that adds its own. Exercises type-param resolution (in-scope `T`
// → `T`), instantiation (`Box<U>`), and optional-return folding (`V | undefined`
// → `V`).

export interface Box<T> {
  value: T;
  get(): T;
  map<U>(f: (v: T) => U): Box<U>;
}

export declare class Container<K, V> {
  constructor(key: K);
  size: number;
  get(key: K): V | undefined;
  set(key: K, value: V): Container<K, V>;
}

export declare function wrap<T>(value: T): Box<T>;
