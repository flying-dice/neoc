--!strict

export type IniParseOptions = {
    comment_symbols: { string }?,
    delimiters: { string }?,
    lowercase_keys: boolean?,
}

export type IniStringifyOptions = {
    spacing: boolean?,
}

export type IniConfig = { [string]: { [string]: string | boolean } }

export type IniModule = {
    parse: ((text: string, options: IniParseOptions?) -> (IniConfig, nil)) & ((text: string, options: IniParseOptions?) -> (nil, string)),
    stringify: ((config: IniConfig, options: IniStringifyOptions?) -> (string, nil)) & ((config: IniConfig, options: IniStringifyOptions?) -> (nil, string)),
}

declare function require(name: "lib:ini"): IniModule

return nil :: any
