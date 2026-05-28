//! The table catalog: column names, types, and nullability collected from the
//! schema's `CREATE TABLE` statements. This is what query inference resolves
//! column references against.

use sqlparser::ast::{ColumnOption, Statement, TableConstraint};

use crate::{ColType, Dialect, Error};

/// One column of a table.
#[derive(Debug, Clone)]
pub(crate) struct Column {
    /// The column name, as written in the schema.
    pub name: String,
    /// The neoc-facing type.
    pub ty: ColType,
    /// `true` when the column is `NOT NULL` or part of the primary key. A
    /// non-null column maps to a bare neoc type; otherwise `Option<T>`.
    pub not_null: bool,
}

/// One table.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    /// The table name, as written in the schema.
    pub name: String,
    /// Columns in declaration order.
    pub columns: Vec<Column>,
}

impl Table {
    /// Finds a column by name, case-insensitively.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }
}

/// Every table the schema declares, keyed by lower-cased name.
#[derive(Debug, Clone, Default)]
pub(crate) struct Catalog {
    tables: Vec<Table>,
}

impl Catalog {
    /// Parses `schema` and collects every `CREATE TABLE`. Non-`CREATE TABLE`
    /// statements are ignored, so a schema dump with indexes and views is fine.
    pub fn from_schema(schema: &str, dialect: Dialect) -> Result<Catalog, Error> {
        let statements = dialect.parse(schema)?;
        let mut tables = Vec::new();
        for stmt in statements {
            if let Statement::CreateTable(create) = stmt {
                tables.push(table_from_create(&create));
            }
        }
        Ok(Catalog { tables })
    }

    /// Finds a table by name, case-insensitively.
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }
}

/// Builds a [`Table`] from a parsed `CREATE TABLE`, folding both column-level
/// `PRIMARY KEY` and a table-level `PRIMARY KEY (…)` constraint into each
/// column's `not_null` flag.
fn table_from_create(create: &sqlparser::ast::CreateTable) -> Table {
    let name = last_ident(&create.name);

    // Table-level `PRIMARY KEY (a, b)` — those columns are implicitly NOT NULL.
    let pk_columns: Vec<String> = create
        .constraints
        .iter()
        .filter_map(|c| match c {
            TableConstraint::PrimaryKey { columns, .. } => {
                Some(columns.iter().map(|i| i.value.clone()))
            }
            _ => None,
        })
        .flatten()
        .collect();

    let columns = create
        .columns
        .iter()
        .map(|col| {
            let mut not_null = false;
            for opt in &col.options {
                match opt.option {
                    ColumnOption::NotNull
                    | ColumnOption::Unique {
                        is_primary: true, ..
                    } => {
                        not_null = true;
                    }
                    _ => {}
                }
            }
            if pk_columns
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&col.name.value))
            {
                not_null = true;
            }
            Column {
                name: col.name.value.clone(),
                ty: ColType::from_sql(&col.data_type),
                not_null,
            }
        })
        .collect();

    Table { name, columns }
}

/// The final segment of a possibly-qualified SQL name (`schema.users` →
/// `users`).
pub(crate) fn last_ident(name: &sqlparser::ast::ObjectName) -> String {
    name.0.last().map(|i| i.value.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Prim;

    #[test]
    fn collects_columns_with_nullability() {
        let schema = "CREATE TABLE users (\
            id INTEGER PRIMARY KEY, \
            name TEXT NOT NULL, \
            email TEXT, \
            active BOOLEAN NOT NULL);";
        let cat = Catalog::from_schema(schema, Dialect::Sqlite).unwrap();
        let users = cat.table("users").unwrap();
        assert_eq!(users.columns.len(), 4);

        let id = users.column("id").unwrap();
        assert!(id.not_null, "primary key is non-null");
        assert_eq!(id.ty.prim, Prim::Number);

        assert!(
            !users.column("email").unwrap().not_null,
            "plain TEXT is nullable"
        );
        assert!(users.column("name").unwrap().not_null);
        assert_eq!(users.column("active").unwrap().ty.prim, Prim::Bool);
    }

    #[test]
    fn table_level_primary_key_is_non_null() {
        let schema = "CREATE TABLE t (a INTEGER, b TEXT, PRIMARY KEY (a));";
        let cat = Catalog::from_schema(schema, Dialect::Sqlite).unwrap();
        assert!(cat.table("t").unwrap().column("a").unwrap().not_null);
    }
}
