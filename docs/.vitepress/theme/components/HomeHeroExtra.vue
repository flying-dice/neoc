<script setup lang="ts">
import { computed, ref } from 'vue'

type Platform = 'unix' | 'windows'
type Example = 'fetch' | 'server' | 'test'

const platform = ref<Platform>('unix')
const example = ref<Example>('fetch')

const installCommands: Record<Platform, string> = {
  unix: 'curl -fsSL https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.sh | bash',
  windows: 'irm https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.ps1 | iex',
}

const promptGlyph: Record<Platform, string> = {
  unix: '$',
  windows: '>',
}

const installCommand = computed(() => installCommands[platform.value])

const exampleSources: Record<Example, { file: string; source: string }> = {
  fetch: {
    file: 'fetch.luau',
    source: `-- Pull from the endpoint, print the response body.
local hyper = require("vnd:hyper")

local resp = hyper.get("http://localhost:3000/")
print(resp.body)`,
  },
  server: {
    file: 'server.luau',
    source: `-- A JSON endpoint, one worker per core, sharing the listener and a counter.
local thread      = require("std:thread")
local TcpListener = require("std:net").TcpListener
local Counter     = require("std:collections").Counter
local workers     = require("std:workers")

local listener = TcpListener.bind("0.0.0.0:3000")
local hits     = Counter.new(0)

for _ = 1, thread.available_parallelism() do
    thread.spawn({
        source = [[
            local hyper  = require("vnd:hyper")
            local json   = require("vnd:serde_json")
            local shared = (require("std:thread")).shared
            hyper.serve(shared.listener, function(req)
                shared.hits:increment()
                return {
                    status  = 200,
                    headers = { ["content-type"] = "application/json" },
                    body    = json.to_string({ ok = true, hits = shared.hits:get() }),
                }
            end)
        ]],
        -- ...or load the worker from a file:
        --   path = "worker.luau",
        shared = { listener = listener, hits = hits },
    })
end

workers.on_shutdown():wait()`,
  },
  test: {
    file: 'users.test.luau',
    source: `-- Spin up Postgres in a container, run a real test, stop it.
local sqlx = require("vnd:sqlx_postgres")
local tc   = require("vnd:testcontainers_modules")
local _t   = require("lib:test")
local describe, test, expect = _t.describe, _t.test, _t.expect

local container = tc.postgres()
local db        = sqlx.open(container:url())

db:execute("CREATE TABLE users (id INT PRIMARY KEY, name TEXT)")
db:execute("INSERT INTO users VALUES ($1, $2)", 1, "alice")

describe("user repository", function()
    test("finds users by id", function()
        local row = db:query_one("SELECT name FROM users WHERE id = $1", 1)
        expect(row.name).toBe("alice")
    end)
end)

db:close()
container:stop()`,
  },
}

const currentExample = computed(() => exampleSources[example.value])

const copied = ref<'' | 'install' | 'script'>('')

function copy(which: 'install' | 'script', text: string) {
  navigator.clipboard.writeText(text).catch(() => {})
  copied.value = which
  setTimeout(() => (copied.value = ''), 1500)
}
</script>

<template>
  <div class="home-hero-extra">
    <p class="hhx-eyebrow">
      <span>Install</span>
    </p>

    <div class="hhx-install-block">
      <div class="hhx-tabs" role="tablist" aria-label="Install platform">
        <button
          type="button"
          role="tab"
          :aria-selected="platform === 'unix'"
          :class="['hhx-tab', { active: platform === 'unix' }]"
          @click="platform = 'unix'"
        >macOS &middot; Linux</button>
        <button
          type="button"
          role="tab"
          :aria-selected="platform === 'windows'"
          :class="['hhx-tab', { active: platform === 'windows' }]"
          @click="platform = 'windows'"
        >Windows</button>
        <span class="hhx-tab-shell">
          {{ platform === 'unix' ? 'bash' : 'PowerShell' }}
        </span>
      </div>

      <button
        class="hhx-install"
        type="button"
        :aria-label="`Copy install command for ${platform === 'unix' ? 'macOS / Linux' : 'Windows'}`"
        @click="copy('install', installCommand)"
      >
        <span class="hhx-glyph">{{ promptGlyph[platform] }}</span>
        <code>{{ installCommand }}</code>
        <span class="hhx-copy">{{ copied === 'install' ? 'Copied' : 'Copy' }}</span>
      </button>
    </div>

    <p class="hhx-eyebrow">
      <span>Run</span>
    </p>

    <div class="hhx-example-block">
      <div class="hhx-tabs" role="tablist" aria-label="Example script">
        <button
          type="button"
          role="tab"
          :aria-selected="example === 'server'"
          :class="['hhx-tab', { active: example === 'server' }]"
          @click="example = 'server'"
        >serve</button>
        <button
          type="button"
          role="tab"
          :aria-selected="example === 'fetch'"
          :class="['hhx-tab', { active: example === 'fetch' }]"
          @click="example = 'fetch'"
        >fetch</button>
        <button
          type="button"
          role="tab"
          :aria-selected="example === 'test'"
          :class="['hhx-tab', { active: example === 'test' }]"
          @click="example = 'test'"
        >test</button>
        <span class="hhx-tab-shell">Luau</span>
      </div>

      <div class="hhx-script">
        <div class="hhx-script-header">
          <span class="hhx-file">{{ currentExample.file }}</span>
          <button
            class="hhx-copy hhx-copy-btn"
            type="button"
            @click="copy('script', currentExample.source)"
          >
            {{ copied === 'script' ? 'Copied' : 'Copy' }}
          </button>
        </div>
        <pre v-if="example === 'fetch'"><code><span class="hhx-com">-- Pull from the endpoint, print the response body.</span>
<span class="hhx-kw">local</span> hyper = <span class="hhx-fn">require</span>(<span class="hhx-str">"vnd:hyper"</span>)

<span class="hhx-kw">local</span> resp = hyper.<span class="hhx-fn">get</span>(<span class="hhx-str">"http://localhost:3000/"</span>)
<span class="hhx-fn">print</span>(resp.body)</code></pre>

        <pre v-else-if="example === 'server'"><code><span class="hhx-com">-- A JSON endpoint, one worker per core, sharing the listener and a counter.</span>
<span class="hhx-kw">local</span> thread      = <span class="hhx-fn">require</span>(<span class="hhx-str">"std:thread"</span>)
<span class="hhx-kw">local</span> TcpListener = <span class="hhx-fn">require</span>(<span class="hhx-str">"std:net"</span>).TcpListener
<span class="hhx-kw">local</span> Counter     = <span class="hhx-fn">require</span>(<span class="hhx-str">"std:collections"</span>).Counter
<span class="hhx-kw">local</span> workers     = <span class="hhx-fn">require</span>(<span class="hhx-str">"std:workers"</span>)

<span class="hhx-kw">local</span> listener = TcpListener.<span class="hhx-fn">bind</span>(<span class="hhx-str">"0.0.0.0:3000"</span>)
<span class="hhx-kw">local</span> hits     = Counter.<span class="hhx-fn">new</span>(<span class="hhx-num">0</span>)

<span class="hhx-kw">for</span> _ = <span class="hhx-num">1</span>, thread.<span class="hhx-fn">available_parallelism</span>() <span class="hhx-kw">do</span>
    thread.<span class="hhx-fn">spawn</span>({
        source = <span class="hhx-str">[[
            local hyper  = require("vnd:hyper")
            local json   = require("vnd:serde_json")
            local shared = (require("std:thread")).shared
            hyper.serve(shared.listener, function(req)
                shared.hits:increment()
                return {
                    status  = 200,
                    headers = { ["content-type"] = "application/json" },
                    body    = json.to_string({ ok = true, hits = shared.hits:get() }),
                }
            end)
        ]]</span>,
<span class="hhx-com">        -- ...or load the worker from a file:
        --   path = "worker.luau",</span>
        shared = { listener = listener, hits = hits },
    })
<span class="hhx-kw">end</span>

workers.<span class="hhx-fn">on_shutdown</span>():<span class="hhx-fn">wait</span>()</code></pre>

        <pre v-else><code><span class="hhx-com">-- Spin up Postgres in a container, run a real test, stop it.</span>
<span class="hhx-kw">local</span> sqlx = <span class="hhx-fn">require</span>(<span class="hhx-str">"vnd:sqlx_postgres"</span>)
<span class="hhx-kw">local</span> tc   = <span class="hhx-fn">require</span>(<span class="hhx-str">"vnd:testcontainers_modules"</span>)
<span class="hhx-kw">local</span> _t   = <span class="hhx-fn">require</span>(<span class="hhx-str">"lib:test"</span>)
<span class="hhx-kw">local</span> describe, test, expect = _t.describe, _t.test, _t.expect

<span class="hhx-kw">local</span> container = tc.<span class="hhx-fn">postgres</span>()
<span class="hhx-kw">local</span> db        = sqlx.<span class="hhx-fn">open</span>(container:<span class="hhx-fn">url</span>())

db:<span class="hhx-fn">execute</span>(<span class="hhx-str">"CREATE TABLE users (id INT PRIMARY KEY, name TEXT)"</span>)
db:<span class="hhx-fn">execute</span>(<span class="hhx-str">"INSERT INTO users VALUES ($1, $2)"</span>, <span class="hhx-num">1</span>, <span class="hhx-str">"alice"</span>)

<span class="hhx-fn">describe</span>(<span class="hhx-str">"user repository"</span>, <span class="hhx-kw">function</span>()
    <span class="hhx-fn">test</span>(<span class="hhx-str">"finds users by id"</span>, <span class="hhx-kw">function</span>()
        <span class="hhx-kw">local</span> row = db:<span class="hhx-fn">query_one</span>(<span class="hhx-str">"SELECT name FROM users WHERE id = $1"</span>, <span class="hhx-num">1</span>)
        <span class="hhx-fn">expect</span>(row.name).<span class="hhx-fn">toBe</span>(<span class="hhx-str">"alice"</span>)
    <span class="hhx-kw">end</span>)
<span class="hhx-kw">end</span>)

db:<span class="hhx-fn">close</span>()
container:<span class="hhx-fn">stop</span>()</code></pre>
      </div>
    </div>
  </div>
</template>
