--!strict
-- vnd:keyring as a script sees it: its answers (keyring.json), then what it asserts. Only mock stores: nothing here
-- reaches the credential store of whoever runs it (`keyring.store()` is not called).
local keyring = require("vnd:keyring")
local out = {}

local store = keyring.mock()

-- a password: kept, replaced, deleted once
local made, entry_err = store:entry("example.com", "ada")
out.entry = made ~= nil and entry_err == nil
local entry = assert(made)
out.set = entry:set_password("first")
out.first = entry:get_password()
out.replaced = { entry:set_password("second"), (entry:get_password()) } :: { any }
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
out.search_empty_spec = #assert(own:search {})
local found = assert(own:search { service = "example", user = "grace" })
out.search_one = { #found, (found[1]:get_password()) } :: { any }
out.search_none = #assert(own:search { service = "nowhere" })

-- what it asserts
do
  local store = keyring.mock()
  local entry = assert(store:entry("example.com", "ada"))

  -- the functions that read an error string are gone: the error says it itself
  assert((keyring :: any).error_kind == nil and (keyring :: any).error_message == nil)

  -- a typed error: a secret that is not text, read as a password
  assert(entry:set_secret("\255hunter2"))
  local password, err = entry:get_password()
  assert(password == nil and err ~= nil and err.kind == "bad_encoding")
  assert(err.op == "vnd:keyring.Entry:get_password")
  assert(err.message == "the secret is not UTF-8 text: read it with `get_secret`")
  assert(
    tostring(err)
      == "vnd:keyring.Entry:get_password: bad_encoding: the secret is not UTF-8 text: read it with `get_secret`"
  )
  assert(not tostring(err):find("hunter2", 1, true))
  assert(entry:get_secret() == "\255hunter2")

  -- the kinds a script branches on, with their fields
  local _, again = entry:get_password()
  if again ~= nil then
    if again.kind == "ambiguous" then
      assert(again.matches > 1)
    elseif again.kind == "no_store" or again.kind == "no_access" or again.kind == "invalid_input" then
      assert(again.reason ~= "")
    else
      assert(again.kind == "bad_encoding")
    end
  end

  -- what is not text is refused as an argument, before the store
  local ok_service, refused_service = pcall(function()
    store:entry("\255", "ada")
  end)
  assert(
    not ok_service
      and tostring(refused_service):find("vnd:keyring.Store:entry: argument #1 'service': expected UTF-8 text", 1, true)
  )
  local ok_user, refused_user = pcall(function()
    store:entry("example.com", 5 :: any) -- deliberate refusal test: a number is not a user
  end)
  assert(not ok_user and tostring(refused_user):find("vnd:keyring.Store:entry: argument #2 'user'", 1, true))
  local ok_password, refused_password = pcall(function()
    entry:set_password("\255")
  end)
  assert(
    not ok_password
      and tostring(refused_password):find(
        "vnd:keyring.Entry:set_password: argument #1 'password': expected UTF-8 text",
        1,
        true
      )
  )
  assert(entry:get_secret() == "\255hunter2")
  local ok_spec, refused_spec = pcall(function()
    store:search({ service = 1 } :: any) -- deliberate refusal test: a spec is text under text
  end)
  assert(not ok_spec and tostring(refused_spec):find("vnd:keyring.Store:search: argument #1 'spec'", 1, true))

  -- success is as it was: true, the value, or nil without an error
  assert(entry:set_password("text") == true)
  assert(entry:set_secret("bytes") == true)
  local service, user = entry:get_specifiers()
  assert(service == "example.com" and user == "ada")
  assert(entry:delete_credential() == true)
  local deleted, delete_err = entry:delete_credential()
  assert(deleted == false and delete_err == nil)
  local gone, gone_err = entry:get_password()
  assert(gone == nil and gone_err == nil)
end

return out
