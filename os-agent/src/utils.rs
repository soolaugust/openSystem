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
