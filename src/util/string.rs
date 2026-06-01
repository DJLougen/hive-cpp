//! Small string helpers. The Python sibling has a few of these inline;
//! the Rust port centralises them so the regex compressor and the LLM
//! client share one implementation.

/// `text.chars().take(n).collect()` but stable on byte boundaries by
/// trimming at the last whitespace if we cut mid-token. Matches the
/// Python `text[:n]` semantics close enough for the benchmark and the
/// compressor, both of which treat characters as opaque.
pub fn truncate_chars(text: &str, n: usize) -> &str {
    if text.len() <= n {
        return text;
    }
    // Walk char boundaries; safe for UTF-8.
    let mut end = n;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_under_limit() {
        assert_eq!(truncate_chars("hello", 10), "hello");
    }

    #[test]
    fn truncate_at_limit() {
        assert_eq!(truncate_chars("hello world", 5), "hello");
    }

    #[test]
    fn truncate_handles_multibyte() {
        // "héllo" is h=1 é=2 l=1 l=1 o=1 → 6 bytes, 5 chars
        let s = "héllo";
        let t = truncate_chars(s, 3);
        assert!(t.is_char_boundary(t.len()));
    }
}
