/// Extract JSON from LLM response, handling markdown code blocks.
/// Supports ```json ... ```, ``` ... ```, and raw JSON objects.
pub fn extract_json(s: &str) -> &str {
    // Handle ```json ... ``` blocks
    if let Some(start) = s.find("```json") {
        let after = &s[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    // Handle ``` ... ``` blocks
    if let Some(start) = s.find("```") {
        let after = &s[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    // Find raw JSON object
    if let Some(start) = s.find('{') {
        if let Some(end) = s.rfind('}') {
            return &s[start..=end];
        }
    }
    s.trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_code_block() {
        let input = r#"Here is the JSON:
```json
{"key": "value"}
```
Done."#;
        assert_eq!(extract_json(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_extract_json_from_plain_code_block() {
        let input = r#"Result:
```
{"key": "value"}
```"#;
        assert_eq!(extract_json(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_extract_json_raw_object() {
        let input = r#"The result is {"action": "noop"} end"#;
        assert_eq!(extract_json(input), r#"{"action": "noop"}"#);
    }

    #[test]
    fn test_extract_json_nested_braces() {
        let input = r#"{"outer": {"inner": "value"}}"#;
        assert_eq!(extract_json(input), r#"{"outer": {"inner": "value"}}"#);
    }

    #[test]
    fn test_extract_json_no_json() {
        let input = "no json here";
        assert_eq!(extract_json(input), "no json here");
    }

    #[test]
    fn test_extract_json_empty_string() {
        assert_eq!(extract_json(""), "");
    }

    #[test]
    fn test_extract_json_whitespace_in_code_block() {
        let input = "```json\n  {\"key\": 1}  \n```";
        assert_eq!(extract_json(input), r#"{"key": 1}"#);
    }

    #[test]
    fn test_extract_json_prefers_json_block_over_plain() {
        let input = "```json\n{\"a\": 1}\n```\n```\n{\"b\": 2}\n```";
        assert_eq!(extract_json(input), r#"{"a": 1}"#);
    }
}
