pub fn qualify_soundpack_id(id: &str, default_prefix: &str) -> String {
    let mut id = id.trim().replace('\\', "/");
    if id.contains("..") || id.contains('\0') || id.starts_with('/') {
        id = id.replace("..", "_").trim_start_matches('/').to_string();
    }
    if id.starts_with("keyboard/") {
        return id;
    }
    format!("{}{}", default_prefix, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_is_qualified_as_a_keyboard_pack() {
        assert_eq!(
            qualify_soundpack_id("eg-oreo", "keyboard/"),
            "keyboard/eg-oreo"
        );
    }

    #[test]
    fn an_already_qualified_id_is_left_alone() {
        assert_eq!(
            qualify_soundpack_id("keyboard/eg-oreo", "keyboard/"),
            "keyboard/eg-oreo"
        );
    }

    #[test]
    fn backslash_ids_are_normalized_rather_than_prefixed() {
        assert_eq!(
            qualify_soundpack_id("keyboard\\eg-oreo", "keyboard/"),
            "keyboard/eg-oreo"
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(
            qualify_soundpack_id("  eg-oreo  ", "keyboard/"),
            "keyboard/eg-oreo"
        );
    }
}
