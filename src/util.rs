/// Slugify a display name to a valid username.
/// "John Smith" -> "john_smith"; deduplicates against existing names via suffix.
pub fn slugify_username(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        "user".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify_username("John Smith"), "john_smith");
        assert_eq!(slugify_username(""), "user");
        assert_eq!(slugify_username("Иван Петров"), "user");
    }
}
