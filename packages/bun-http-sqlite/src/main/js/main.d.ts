import type { Result, Option } from "./neoc-lib";
import type { SqlExecutor } from "./autosql/ports";
import type { migrate } from "./autosql/migrate";
import type { add_note } from "./autosql/add_note";
import type { ListNotesRow } from "./autosql/list_notes";
import type { list_notes } from "./autosql/list_notes";

export declare function init<E>(db: E): Result<number, string>;
export declare function list<E>(db: E): Result<ListNotesRow[], string>;
export declare function create<E>(db: E, body: string): Result<number, string>;
