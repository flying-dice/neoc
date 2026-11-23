//! The declarations of a module surface as a tree: what a runtime offers a script, said once and in no language.
//!
//! A [`Section`] is one module (`std:fs`): its types, each an alias of a [`Ty`], the last of them the type of the
//! module's own table, and the globals it declares. neoc-vm builds the tree from what each module records as it
//! registers its functions (`neoc_vm::lua::facade`) and writes it to `types/neoc.ast.json`. From the tree:
//!
//! - [`luau`] writes the Luau declarations (`types/neoc.d.luau`, for luau-lsp);
//! - a generator for another language reads the JSON and writes its own (vos-codegen writes TypeScript's).
//!
//! [`parse`] reads Luau's type syntax into the tree: one recorded type (`{ [string]: number }?`), or a whole
//! definitions file that was written by hand.
//!
//! A function whose first parameter is named `self` is a method (called with `:`); any other is called with `.`.
//! A name is kept as the runtime has it: what a language cannot write as it is (a reserved word) is that language's
//! writer's to escape.

use serde::{Deserialize, Serialize};

pub mod luau;
pub mod parse;

/// A type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ty {
    /// `string`, `nil`, `any`, the alias of a declared type (`std_fs_Metadata`), a generic's `T`.
    Name(String),
    /// A string literal type, as Luau writes it, with its quotes and its escapes: `"user"`.
    Str(String),
    Bool(bool),
    /// `T?`
    Optional(Box<Ty>),
    Union(Vec<Ty>),
    Inter(Vec<Ty>),
    /// `{ T }`
    Array(Box<Ty>),
    /// `{ name: T, [K]: V }`
    Table {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        props: Vec<Prop>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<Box<(Ty, Ty)>>,
    },
    Func(Box<Func>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Func {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
    /// `...T` after the parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<Ty>,
    pub ret: Ret,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ty: Ty,
}

/// What a function returns: no value (`()`), one, or several, and `...T` after them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ret {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Ty>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<Ty>,
}

/// A property of a table type, with the lines of its doc joined by `\n`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prop {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    pub name: String,
    pub ty: Ty,
}

/// A named type (`export type <name> = <ty>` in Luau) or a global (`declare <name>: <ty>`), with its doc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decl {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    pub name: String,
    pub ty: Ty,
}

/// One module's declarations (in a Luau definitions file, what follows one `-- ===== <name> =====` line).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// The module's key: `std:fs`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<Decl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub globals: Vec<Decl>,
}

/// The name of a method's receiver: a function whose first parameter has it is called with `:`.
pub const SELF: &str = "self";
