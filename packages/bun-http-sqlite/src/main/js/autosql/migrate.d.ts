import type { Result, Option } from "../neoc-lib";
import type { SqlExecutor } from "./ports";

export declare function migrate<E>(db: E): Result<number, string>;
