//! ManageSieve RFC 5804 Protocol Client & Sieve Syntax Validator §7 Phase 5
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SieveScript {
    pub name: String,
    pub content: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SieveResponse {
    Ok(Option<String>),
    No(String),
    Bye(String),
}

/// Sieve Syntax Validator & AST sanity checker
pub struct SieveValidator;

impl SieveValidator {
    /// Validates basic Sieve script syntax (RFC 5228)
    pub fn validate(script: &str) -> Result<(), String> {
        let mut brace_depth: i32 = 0;
        let mut in_string = false;
        let mut has_require = false;
        let mut valid_actions = false;

        let trimmed = script.trim();
        if trimmed.is_empty() {
            return Err("Sieve script is empty".into());
        }

        let action_keywords = ["fileinto", "redirect", "reject", "discard", "keep", "stop"];

        for line in trimmed.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.starts_with('#') {
                continue; // Comment line
            }

            if line_trimmed.starts_with("require") {
                has_require = true;
            }

            for action in action_keywords {
                if line_trimmed.contains(action) {
                    valid_actions = true;
                    break;
                }
            }

            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '\\' if in_string => {
                        // Skip next character if escaped within string literal
                        chars.next();
                    }
                    '"' => in_string = !in_string,
                    '{' if !in_string => brace_depth += 1,
                    '}' if !in_string => {
                        brace_depth -= 1;
                        if brace_depth < 0 {
                            return Err("Unmatched closing brace '}' in Sieve script".into());
                        }
                    }
                    _ => {}
                }
            }
        }

        if brace_depth != 0 {
            return Err(format!("Unbalanced braces: {brace_depth} unclosed blocks"));
        }

        if in_string {
            return Err("Unclosed string literal in Sieve script".into());
        }

        if !has_require && !valid_actions {
            return Err(
                "Sieve script does not contain any valid actions or require statements".into(),
            );
        }

        Ok(())
    }
}

/// ManageSieve RFC 5804 command serializer
pub struct ManageSieveCommand;

impl ManageSieveCommand {
    fn sanitize_name(name: &str) -> String {
        name.replace(['\r', '\n', '"', '\\', '/', ';', '{', '}'], "")
    }

    pub fn put_script(name: &str, content: &str) -> String {
        let clean = Self::sanitize_name(name);
        let bytes = content.len();
        format!("PUTSCRIPT \"{clean}\" {{{bytes}+}}\r\n{content}\r\n")
    }

    pub fn get_script(name: &str) -> String {
        let clean = Self::sanitize_name(name);
        format!("GETSCRIPT \"{clean}\"\r\n")
    }

    pub fn set_active(name: &str) -> String {
        let clean = Self::sanitize_name(name);
        format!("SETACTIVE \"{clean}\"\r\n")
    }

    pub fn list_scripts() -> &'static str {
        "LISTSCRIPTS\r\n"
    }

    pub fn delete_script(name: &str) -> String {
        let clean = Self::sanitize_name(name);
        format!("DELETESCRIPT \"{clean}\"\r\n")
    }

    pub fn check_space(name: &str, size_bytes: usize) -> String {
        let clean = Self::sanitize_name(name);
        format!("HAVESPACE \"{clean}\" {size_bytes}\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sieve_validator_valid_script() {
        let valid = r#"
require ["fileinto", "reject"];

if header :contains "subject" "spam" {
    fileinto "Junk";
    stop;
}
"#;
        assert!(SieveValidator::validate(valid).is_ok());
    }

    #[test]
    fn test_sieve_validator_unbalanced_braces() {
        let invalid = r#"
require ["fileinto"];
if header :contains "subject" "spam" {
    fileinto "Junk";
"#;
        let res = SieveValidator::validate(invalid);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Unbalanced braces"));
    }

    #[test]
    fn test_managesieve_command_builders() {
        let script = "keep;";
        let cmd = ManageSieveCommand::put_script("default", script);
        assert_eq!(cmd, "PUTSCRIPT \"default\" {5+}\r\nkeep;\r\n");
        assert_eq!(
            ManageSieveCommand::set_active("default"),
            "SETACTIVE \"default\"\r\n"
        );
        assert_eq!(ManageSieveCommand::list_scripts(), "LISTSCRIPTS\r\n");
    }

    #[test]
    fn test_sieve_validator_escaped_quotes() {
        let script = r#"
require ["fileinto"];
if header :contains "subject" "hello \"world\" test" {
    fileinto "Inbox";
    stop;
}
"#;
        assert!(SieveValidator::validate(script).is_ok());
    }
}
