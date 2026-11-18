import { Database } from 'bun:sqlite'
import type { AppManifest } from '@agentos/sdk'

export interface InstalledRow {
  id: string
  version: string
  registry: string
  manifest: AppManifest
  serverSha: string
  clientSha: string
  installedAt: number
}

/**
 * The server's own SQLite database (`<data>/agentos.sqlite`): which apps are
 * installed, and every app's key/value storage. Apps that want real SQL open
 * their own database in `ctx.dataDir`.
 */
export class SystemDb {
  readonly #db: Database

  constructor(path: string) {
    this.#db = new Database(path, { create: true, strict: true })
    this.#db.exec('PRAGMA journal_mode = WAL')
    this.#db.exec(`
      CREATE TABLE IF NOT EXISTS installed_apps (
        id TEXT PRIMARY KEY,
        version TEXT NOT NULL,
        registry TEXT NOT NULL,
        manifest TEXT NOT NULL,
        server_sha TEXT NOT NULL,
        client_sha TEXT NOT NULL,
        installed_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS app_kv (
        app_id TEXT NOT NULL,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        PRIMARY KEY (app_id, key)
      );
    `)
  }

  installed(): InstalledRow[] {
    const rows = this.#db
      .query<
        { id: string; version: string; registry: string; manifest: string; server_sha: string; client_sha: string; installed_at: number },
        []
      >('SELECT * FROM installed_apps ORDER BY installed_at')
      .all()
    return rows.map((r) => ({
      id: r.id,
      version: r.version,
      registry: r.registry,
      manifest: JSON.parse(r.manifest) as AppManifest,
      serverSha: r.server_sha,
      clientSha: r.client_sha,
      installedAt: r.installed_at
    }))
  }

  saveInstalled(row: InstalledRow): void {
    this.#db
      .query(
        `INSERT INTO installed_apps (id, version, registry, manifest, server_sha, client_sha, installed_at)
         VALUES ($id, $version, $registry, $manifest, $serverSha, $clientSha, $installedAt)
         ON CONFLICT(id) DO UPDATE SET version = $version, registry = $registry, manifest = $manifest,
           server_sha = $serverSha, client_sha = $clientSha, installed_at = $installedAt`
      )
      .run({ ...row, manifest: JSON.stringify(row.manifest) })
  }

  removeInstalled(id: string): void {
    this.#db.query('DELETE FROM installed_apps WHERE id = ?').run(id)
  }

  kvGet(appId: string, key: string): unknown {
    const row = this.#db.query<{ value: string }, [string, string]>('SELECT value FROM app_kv WHERE app_id = ? AND key = ?').get(appId, key)
    return row ? (JSON.parse(row.value) as unknown) : undefined
  }

  kvSet(appId: string, key: string, value: unknown): void {
    if (value === undefined) {
      this.#db.query('DELETE FROM app_kv WHERE app_id = ? AND key = ?').run(appId, key)
      return
    }
    this.#db
      .query('INSERT INTO app_kv (app_id, key, value) VALUES (?, ?, ?) ON CONFLICT(app_id, key) DO UPDATE SET value = excluded.value')
      .run(appId, key, JSON.stringify(value))
  }

  close(): void {
    this.#db.close()
  }
}
