--!strict
-- The keychain of the machine the host runs on, as this app uses it: the one place that talks to the operating
-- system's credential store (through `vnd:keyring`) and says in the app's own words what went wrong. That is the
-- login keychain on macOS, the Credential Manager on Windows and the Secret Service (GNOME Keyring, KWallet) on Linux.
--
-- A password is named by what it is for (the service) and whose it is there (the account). The operating system may
-- ask the person at the machine before it hands one over, and a call waits for as long as they take.
--
-- A function that does something answers with a plain sentence when it could not (the handler and the tool pass it on
-- as it is) and with nothing when it did what it was asked.
local keyring = require("vnd:keyring")
local rpc = require("../.app/rpc")

local vault = {}

-- Why a call failed, as a sentence for the person. The store's own message is kept for what the app has no words for.
local function sentence(err: string?): string
  local kind = keyring.error_kind(err)
  if kind == "no_store" then
    return "This machine has no keychain the app can use. On Linux it needs a Secret Service, like GNOME Keyring or KWallet, to be running."
  elseif kind == "no_access" then
    return "The keychain is locked, or access to it was refused."
  elseif kind == "ambiguous" then
    return "The keychain has more than one item for this service and account."
  elseif kind == "bad_encoding" then
    return "This item's secret is not text, so it cannot be shown."
  end
  return keyring.error_message(err) or err or "The keychain refused, and did not say why."
end

local function entry(service: string, account: string): (vnd_keyring_Entry?, string?)
  local store, storeErr = keyring.store()
  if not store then
    return nil, sentence(storeErr)
  end
  local found, err = store:entry(service, account)
  if not found then
    return nil, sentence(err)
  end
  return found, nil
end

-- whether this machine has a keychain that opens, and why not
function vault.status(): rpc.Keychain
  local store, err = keyring.store()
  return { available = store ~= nil, problem = if store then "" else sentence(err) }
end

-- The password kept for a service and an account. Neither a password nor a sentence: the keychain has no such item.
function vault.read(service: string, account: string): (string?, string?)
  local found, problem = entry(service, account)
  if not found then
    return nil, problem
  end
  local password, err = found:get_password()
  if err then
    return nil, sentence(err)
  end
  return password, nil
end

-- keep a password, in place of the one that was there
function vault.write(service: string, account: string, password: string): string?
  local found, problem = entry(service, account)
  if not found then
    return problem
  end
  local _, err = found:set_password(password)
  return if err then sentence(err) else nil
end

-- delete an item; one the keychain does not have is deleted already
function vault.delete(service: string, account: string): string?
  local found, problem = entry(service, account)
  if not found then
    return problem
  end
  local _, err = found:delete_credential()
  return if err then sentence(err) else nil
end

return vault
