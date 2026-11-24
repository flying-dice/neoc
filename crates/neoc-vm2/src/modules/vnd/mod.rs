//! `vnd:*` modules: vendored crates, each named after its crate. A module is a `#[mlua_ir::module("vnd:<name>")]`
//! inside its file; its `pub mod` line here is what compiles and links it.

pub mod hyper;
pub mod jsonschema;
pub mod keyring;
pub mod quick_xml;
pub mod rusqlite;
pub mod serde_json;
pub mod serde_yaml;
pub mod sqlx_mysql;
pub mod sqlx_postgres;
pub mod toml;

/// The most placeholders a statement of the network databases has (MySQL's and Postgres's protocols count them in
/// 16 bits), and so the highest position a parameter is read at.
const MAX_PARAMETERS: u32 = 65_535;

/// A statement's parameters in order, from what a script gave by position (from 1): position 1 first, up to the
/// highest position given, and `None` for a position without a value. `Err` is a position no statement has: 0, or
/// one past [`MAX_PARAMETERS`].
///
/// For the drivers that do not say how many placeholders a statement has before it runs (`sqlx_mysql`,
/// `sqlx_postgres`); `rusqlite` asks the statement.
fn by_position<P>(params: Option<std::collections::BTreeMap<u32, Option<P>>>) -> Result<Vec<Option<P>>, u32> {
    let mut values = Vec::new();
    for (position, param) in params.into_iter().flatten() {
        if position == 0 || position > MAX_PARAMETERS {
            return Err(position);
        }
        // the map is in key order: every position before this one that was not given is a `None`
        values.resize_with(position as usize - 1, || None);
        values.push(param);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{MAX_PARAMETERS, by_position};

    #[test]
    fn positions_come_out_in_order_with_the_gaps_empty() {
        assert_eq!(by_position::<u8>(None), Ok(vec![]));
        assert_eq!(by_position(Some(BTreeMap::from([(1, Some('a')), (2, Some('b'))]))), Ok(vec![Some('a'), Some('b')]));
        assert_eq!(by_position(Some(BTreeMap::from([(3, Some('c')), (1, Some('a'))]))), Ok(vec![Some('a'), None, Some('c')]));
        assert_eq!(by_position(Some(BTreeMap::from([(2, None), (1, Some('a'))]))), Ok(vec![Some('a'), None]));
    }

    #[test]
    fn a_position_no_statement_has_is_refused() {
        assert_eq!(by_position(Some(BTreeMap::from([(0, Some('a'))]))), Err(0));
        assert_eq!(by_position(Some(BTreeMap::from([(MAX_PARAMETERS + 1, Some('a'))]))), Err(MAX_PARAMETERS + 1));
        assert_eq!(by_position(Some(BTreeMap::from([(MAX_PARAMETERS, Some('a'))]))).map(|values| values.len()), Ok(65_535));
    }
}
