--!strict
-- intentional: what a row is no longer depends on an option (parse / rows give lists, parse_records / records give records keyed by header; they were parse / rows with headers = true), rows / records return a stream (rows was a bare iterator), the header list is a parameter of stringify and writer (it was a required option) and stringify takes positional rows too, failures are typed (CsvError, WriteError), wrong options and rows are refused as arguments, a types entry for a column the header lacks is an error, and csv.reader(handle) has no headers option either (rows gives the rows not read yet as lists, headers reads the next row as the header row, records gives the rows after it keyed by header; a handle that is not a std:io reader is refused as an argument).
local csv = require("lib:csv")

-- the types of columns are annotated where they are written: the checker does not read a column's type from
-- a literal nested in the options
type Types = { [string]: lib_csv_ColumnType }

-- records, typed columns
local people_types: Types = { age = "number", admin = "boolean" }
local people = assert(csv.parse_records("name,age,admin\nada,36,true\ngrace,,0", { types = people_types }))
assert(#people == 2 and people[1].name == "ada" and people[1].age == 36 and people[1].admin == true)
assert(people[2].age == nil and people[2].admin == false)

-- a std:io reader, read as it comes
local io = require("std:io")
local handle = io.from_string("name,age\nada,36\ngrace,45\n")
local age_types: Types = { age = "number" }
local reader = assert(csv.reader(handle, { types = age_types }))
local busy, in_use = csv.reader(handle)
assert(busy == nil and in_use ~= nil and in_use.kind == "in_use" and in_use.op == "lib:csv.reader")
assert(table.concat(assert(reader:headers()), "|") == "name|age")
local ages = 0
for person in reader:records():items() do
	ages += person.age :: number
end
assert(ages == 81 and reader:rows():next() == nil)
local lists = assert(csv.reader(io.from_string("a,b\n1,2")))
local every = {}
for list in lists:rows():items() do
	table.insert(every, table.concat(list, "+"))
end
assert(table.concat(every, " ") == "a+b 1+2")
local uneven = assert(csv.reader(io.from_string('a,b\n"x",y,extra\n')))
local _, mismatch = uneven:records():next()
assert(mismatch ~= nil and mismatch.kind == "unequal_lengths" and mismatch.text == '"x",y,extra' and mismatch.op == "lib:csv.Reader:records")
local not_a_reader, refused_handle = pcall(function()
	csv.reader("not a handle" :: any) -- deliberate refusal test: text is not a handle
end)
assert(not not_a_reader and tostring(refused_handle):find("lib:csv.reader: argument #1 'handle': expected a Reader", 1, true))

-- streams
local seen = {}
for row in csv.rows("a,b\n1,2"):items() do
	table.insert(seen, table.concat(row, "+"))
end
assert(table.concat(seen, " ") == "a+b 1+2")
local n_types: Types = { n = "number" }
local stream = csv.records("n\n1\n2", { types = n_types })
local first = assert(stream:next())
assert(first.n == 1)
stream:close()
assert(stream:next() == nil)
local failing = csv.rows("a,b\n1")
assert((assert(failing:next()))[1] == "a")
local row, why = failing:next()
assert(row == nil and why ~= nil and why.kind == "unequal_lengths" and why.op == "lib:csv.rows")
local raised, caught = pcall(function()
	for _ in csv.records("a,a\n1,2"):items() do
	end
end)
assert(not raised and tostring(caught):find("lib:csv.records: duplicate_header:", 1, true))

-- typed errors
local _, unequal = csv.parse('a,b,c\n"x",y,z,extra\n')
assert(unequal ~= nil and unequal.kind == "unequal_lengths" and unequal.line == 2 and unequal.found == 4 and unequal.expected == 3)
assert(unequal.text == '"x",y,z,extra' and unequal.op == "lib:csv.parse")
assert(tostring(unequal) == 'lib:csv.parse: unequal_lengths: line 2: found 4 fields, expected 3: ""x",y,z,extra"')
local _, duplicate = csv.parse_records("name,age,name\nada,36,x")
assert(duplicate ~= nil and duplicate.kind == "duplicate_header" and duplicate.header == "name" and duplicate.text == "name,age,name")
local dur_types: Types = { dur = "number" }
local _, coerce = csv.parse_records("dur\nabc", { types = dur_types })
assert(coerce ~= nil and coerce.kind == "cannot_coerce" and coerce.line == 2 and coerce.column == "dur" and coerce.value == "abc")
assert(coerce.expected == "number")
local b_types: Types = { b = "number" }
local _, unknown = csv.parse_records("a\n1", { types = b_types })
assert(unknown ~= nil and unknown.kind == "unknown_column" and unknown.column == "b")
local _, not_text = csv.parse_records("a\255\n1")
assert(not_text ~= nil and not_text.kind == "invalid_utf8" and not_text.line == 1)

-- writing: the headers are a parameter, rows are lists or records
local by_name: { [string]: lib_csv_Cell } = { name = "ada", age = 36 }
local by_place: { lib_csv_Cell } = { "grace", 45 }
local text = assert(csv.stringify({ by_name, by_place, { name = "a,b" }, {} }, { "name", "age" }))
assert(text == 'name,age\nada,36\ngrace,45\n"a,b",\n,\n')
local mixed: { lib_csv_Cell } = { "x", true }
assert(csv.stringify({ mixed }, { "a", "b" }, { delimiter = "\t" }) == "a\tb\nx\ttrue\n")
local w = assert(csv.writer({ "test", "status" }))
assert(w:row({ "login", "PASS" }) == true and w:row({ test = "signup" }) == true)
local _, short = w:row({ "only one" })
assert(short ~= nil and short.kind == "wrong_length" and short.found == 1 and short.expected == 2 and short.op == "lib:csv.Writer:row")
assert(w:finish() == "test,status\nlogin,PASS\nsignup,\n")
local _, finished = w:finish()
local _, late = w:row({ "a", "b" })
assert(finished ~= nil and late ~= nil)
assert(finished.kind == "finished" and late.kind == "finished")
local _, none = csv.writer({})
assert(none ~= nil and none.kind == "no_headers")

-- arguments are checked by their types
local function accepted(call: () -> ...unknown): boolean
	return (pcall(call))
end
local ok, refused = pcall(function()
	csv.parse("a,b", { delimiter = ",," })
end)
assert(not ok and tostring(refused):find("lib:csv.parse: argument #2 'opts'", 1, true))
-- deliberate refusal tests: options and rows of the wrong shape
assert(not accepted(function()
	csv.parse("a,b", { headers = true } :: any)
end))
assert(not accepted(function()
	csv.parse("a,b", { types = { a = "number" } } :: any)
end))
assert(not accepted(function()
	csv.parse_records("a\n1", { types = { a = "integer" } } :: any)
end))
local ok2, refused2 = pcall(function()
	w:row({ "x", b = "y" } :: any)
end)
assert(not ok2 and tostring(refused2):find("lib:csv.Writer:row: argument #1 'values'", 1, true))
assert(not accepted(function()
	csv.stringify({ { a = {} } } :: any, { "a" })
end))
