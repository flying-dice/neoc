import type { Result, Option } from "./neoc-lib";
import type { SqlExecutor } from "./autosql/ports";
import type { migrate } from "./autosql/migrate";
import type { add_note } from "./autosql/add_note";
import type { ListNotesRow } from "./autosql/list_notes";
import type { list_notes } from "./autosql/list_notes";

export interface HttpResponse { status: number; body: string; }
export declare function init<E extends SqlExecutor>(db: E): Result<number, string>;
export declare function list<E extends SqlExecutor>(db: E): HttpResponse;
export declare function create<E extends SqlExecutor>(db: E, body: string): HttpResponse;
