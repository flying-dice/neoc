//! What a label, a spec and a patch may say, checked before any manager is asked: what the types do not say and a
//! service definition must not hold (a line break would write a second setting into a unit file).

use std::collections::BTreeMap;
use std::path::Path;

use super::service_manager::{Error, Patch, Spec};

const MAX_LABEL: usize = 200;
const MAX_TEXT: usize = 4096;
const MAX_ITEMS: usize = 1024;
const MAX_DELAY_SECS: u32 = 86_400;

fn invalid(reason: String) -> Error {
    Error::InvalidInput { reason }
}

/// A label is the manager's own name for a service: letters, digits and `_ . - @ :`, starting with a letter, a digit
/// or `_` (so it can never be read as an option of a command).
pub(super) fn label(label: &str) -> Result<(), Error> {
    let first_ok = label.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    let rest_ok =
        label.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '@' | ':'));
    if label.is_empty() || label.len() > MAX_LABEL || !first_ok || !rest_ok {
        return Err(invalid(format!(
            "label {label:?} must be 1 to {MAX_LABEL} letters, digits or `_ . - @ :`, starting with a letter, a digit or `_`"
        )));
    }
    Ok(())
}

/// Text that goes into a definition: no control characters (a line break, a NUL) and not long.
fn text(option: &str, value: &str) -> Result<(), Error> {
    if value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "option \"{option}\" cannot contain a control character such as a line break"
        )));
    }
    if value.len() > MAX_TEXT {
        return Err(invalid(format!("option \"{option}\" holds text longer than {MAX_TEXT} bytes")));
    }
    Ok(())
}

fn program(value: &str) -> Result<(), Error> {
    text("program", value)?;
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(invalid(format!("option \"program\" must be an absolute path, not {value:?}")));
    }
    let meta = std::fs::metadata(path)
        .map_err(|e| invalid(format!("option \"program\" {value:?} cannot be used: {e}")))?;
    if !meta.is_file() {
        return Err(invalid(format!("option \"program\" {value:?} is not a file")));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err(invalid(format!("option \"program\" {value:?} is not executable")));
        }
    }
    Ok(())
}

fn args(value: &[String]) -> Result<(), Error> {
    if value.len() > MAX_ITEMS {
        return Err(invalid(format!("option \"args\" holds more than {MAX_ITEMS} entries")));
    }
    value.iter().try_for_each(|arg| text("args", arg))
}

fn directory(value: &str) -> Result<(), Error> {
    text("working_directory", value)?;
    let path = Path::new(value);
    if !path.is_absolute() || !path.is_dir() {
        return Err(invalid(format!(
            "option \"working_directory\" {value:?} must be an absolute path of a directory"
        )));
    }
    Ok(())
}

fn environment(value: &BTreeMap<String, String>) -> Result<(), Error> {
    if value.len() > MAX_ITEMS {
        return Err(invalid(format!("option \"environment\" holds more than {MAX_ITEMS} entries")));
    }
    for (name, v) in value {
        let mut chars = name.chars();
        let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            return Err(invalid(format!(
                "option \"environment\" has the name {name:?}: letters, digits and `_`, not starting with a digit"
            )));
        }
        text("environment", v)?;
    }
    Ok(())
}

fn delay(value: u32) -> Result<(), Error> {
    if value > MAX_DELAY_SECS {
        return Err(invalid(format!("option \"restart_delay_secs\" must be at most {MAX_DELAY_SECS}")));
    }
    Ok(())
}

fn user(value: &str) -> Result<(), Error> {
    let ok = !value.is_empty()
        && value.len() <= 64
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '\\' | '$'));
    if ok { Ok(()) } else { Err(invalid(format!("option \"user\" {value:?} is not a user name"))) }
}

pub(super) fn spec(spec: &Spec) -> Result<(), Error> {
    label(&spec.label)?;
    program(&spec.program)?;
    if let Some(v) = &spec.args {
        args(v)?;
    }
    if let Some(v) = &spec.description {
        text("description", v)?;
    }
    if let Some(v) = &spec.working_directory {
        directory(v)?;
    }
    if let Some(v) = &spec.environment {
        environment(v)?;
    }
    if let Some(v) = spec.restart_delay_secs {
        delay(v)?;
    }
    if let Some(v) = &spec.user {
        user(v)?;
    }
    Ok(())
}

pub(super) fn patch(patch: &Patch) -> Result<(), Error> {
    if patch.supplied().is_empty() {
        return Err(invalid("a change must set at least one option".to_owned()));
    }
    if let Some(v) = &patch.program {
        program(v)?;
    }
    if let Some(v) = &patch.args {
        args(v)?;
    }
    if let Some(v) = &patch.description {
        text("description", v)?;
    }
    if let Some(v) = &patch.working_directory {
        directory(v)?;
    }
    if let Some(v) = &patch.environment {
        environment(v)?;
    }
    if let Some(v) = patch.restart_delay_secs {
        delay(v)?;
    }
    Ok(())
}
