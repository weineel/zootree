use anyhow::Result;

pub fn validate_config_name(kind: &str, name: &str) -> Result<()> {
    if is_config_name(name) {
        return Ok(());
    }

    anyhow::bail!("invalid {kind} name {name:?}: use only ASCII letters, numbers, '-' and '_'")
}

pub fn is_config_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
