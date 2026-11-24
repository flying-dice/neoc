-- vnd:keyring on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Only mock stores: nothing here reaches the credential store of whoever runs it (`keyring.store()` is not called).
-- What an error is, and what is refused as an argument, differ on purpose: see keyring.vm2.luau.
local keyring = require("vnd:keyring")
local out = {}

local store = keyring.mock()

-- a password: kept, replaced, deleted once
local entry, entry_err = store:entry("example.com", "ada")
out.entry = entry ~= nil and entry_err == nil
out.set = entry:set_password("first")
out.first = entry:get_password()
out.replaced = { entry:set_password("second"), (entry:get_password()) }
out.deleted = entry:delete_credential()
local after, after_err = entry:get_password()
out.after_delete = { after == nil, after_err == nil }
local again, again_err = entry:delete_credential()
out.deleted_again = { again, again_err == nil }

-- no credential is an answer, not an error
local nobody = assert(store:entry("example.com", "nobody"))
local password, password_err = nobody:get_password()
local secret, secret_err = nobody:get_secret()
out.absent = { password == nil, password_err == nil, secret == nil, secret_err == nil }

-- two entries of one name are one credential; another store is apart
assert(assert(store:entry("example.com", "shared")):set_password("one"))
out.shared = assert(store:entry("example.com", "shared")):get_password()
out.other_store = assert(keyring.mock():entry("example.com", "shared")):get_password() == nil

-- a secret is the bytes that were stored
local key = assert(store:entry("example.com", "key"))
out.set_secret = key:set_secret("\0\255bytes\0")
local bytes = assert(key:get_secret())
out.secret = { #bytes, string.byte(bytes, 1, -1) }
-- text stored as a password reads back as a secret, and the other way
out.password_as_secret = assert(store:entry("example.com", "shared")):get_secret()
assert(key:set_secret("plain"))
out.secret_as_password = key:get_password()
-- a secret that is not text is an error as a password
assert(key:set_secret("\255hunter2"))
local not_text, not_text_err = key:get_password()
out.bad_encoding = not_text == nil and not_text_err ~= nil

-- an entry says what it was made from
local service, user = entry:get_specifiers()
out.specifiers = { service, user }

-- search
local own = keyring.mock()
assert(assert(own:entry("example.com", "ada")):set_password("a"))
assert(assert(own:entry("example.com", "grace")):set_password("g"))
assert(assert(own:entry("other.org", "ada")):set_password("o"))
out.search_all = #assert(own:search())
out.search_empty_spec = #assert(own:search({}))
local found = assert(own:search({ service = "example", user = "grace" }))
out.search_one = { #found, (found[1]:get_password()) }
out.search_none = #assert(own:search({ service = "nowhere" }))

return out
