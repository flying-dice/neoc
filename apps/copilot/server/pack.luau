--!strict
-- Files made from text, as a tar archive Docker takes: a build context (a copilot's Dockerfile) and the skills
-- unpacked into a chat's container. `lib:archive` makes a tar from files on disk, so each file is written into a
-- temporary folder first, under a name of its own, and named in the archive by its path there.
local archive = require("lib:archive")
local fs = require("std:fs")
local tempfile = require("lib:tempfile")

-- A file of the archive: its path there, with `/` between its parts, and what it holds.
export type File = { path: string, text: string }

local pack = {}

-- The files as a tar archive (uncompressed), or why it could not be made.
function pack.tar(files: { File }): (string?, string?)
  local folder, made = tempfile.tempdir { prefix = "vos-copilot-" }
  if not folder then
    return nil, `A temporary folder could not be made: {if made then made.message else "it did not say why"}`
  end
  local entries: { lib_archive_TarEntry } = {}
  for index, file in files do
    local disk = `{folder:path()}/{index}`
    local written, failed = fs.write(disk, file.text)
    if not written then
      folder:close()
      return nil, `A temporary file could not be written: {if failed then failed.message else "it did not say why"}`
    end
    table.insert(entries, { path = disk, archive_path = file.path })
  end
  local bytes, refused = archive.Tar.create(entries)
  folder:close()
  if not bytes then
    return nil, `The archive could not be made: {if refused then refused.message else "it did not say why"}`
  end
  return bytes, nil
end

return pack
