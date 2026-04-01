/// Generate a new ULID as a lowercase string.
pub fn new_id() -> String {
    ulid::Ulid::new().to_string().to_lowercase()
}

/// Return the first 8 characters of an ID for display purposes.
pub fn short_id(id: &str) -> &str {
    if id.len() >= 8 {
        &id[..8]
    } else {
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_is_lowercase_ulid() {
        let id = new_id();
        assert_eq!(id.len(), 26);
        assert_eq!(id, id.to_lowercase());
    }

    #[test]
    fn short_id_truncates() {
        assert_eq!(short_id("01abcdef99999999999999999"), "01abcdef");
    }

    #[test]
    fn short_id_short_input() {
        assert_eq!(short_id("abc"), "abc");
    }
}
