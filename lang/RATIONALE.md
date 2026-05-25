# RATIONALE

**neoc is a programming language.** Its surface — `struct`, `trait`, `impl`, `match`, `Result`, the `?` operator, generics, attributes — comes from Rust. Its output is Lua 5.1. Its boundary with the runtime that executes it is a single keyword: `ext`.

## Why

Lua and its dialects are embedded in production software because they are small, fast, and easy to host. The programs that grow inside those hosts deserve the structural tools larger codebases use — a real type system, algebraic data, pattern matching. neoc is that toolset, sitting in front of the Lua runtime that already exists.

## Why Rust's surface

Of the structured languages with a current ecosystem, Rust has the most expressive surface for modelling values: structs and trait impls, sum types with pattern matching, errors as values via `Result` and `?`, generics that mean what they appear to mean. It is also the language an increasing share of system tooling already uses, so the surface is familiar to the people most likely to reach for neoc.

## Why Lua 5.1

It is the lowest common denominator. The same emitted output runs on Luau (the embedded `neoc` runtime), on stock Lua 5.1 through 5.4, and on LuaJIT. One target, many runtimes.

## Why a separate spec

neoc is defined by the reference manual in `lang/`. Any implementation in any host language must satisfy the same spec. The implementation is in Rust today; the language is not.

## Why `ext`

Runtimes plug their APIs into neoc through `ext` declarations written in neoc. One mechanism, visible and greppable in source, that lets embedders expose whatever they want without coupling the language to any one runtime.
