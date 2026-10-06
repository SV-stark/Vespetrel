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

/// Strip a trailing RFC 5228 §2.3 comment, respecting string literals.
///
/// A `#` inside a quoted string is data, not a comment marker.
fn strip_sieve_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'#' if !in_string => return &line[..i],
            _ => {}
        }
    }
    line
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
            // RFC 5228 §2.3: a `#` outside a string literal starts a comment that runs to
            // end of line. The old code only skipped lines *starting* with `#`, so braces
            // and keywords inside a trailing comment were counted. That both rejected
            // valid scripts (`fileinto "Junk";   # move it }`) and accepted invalid ones
            // (`if ... {   # }`).
            let code = strip_sieve_comment(line);
            let line_trimmed = code.trim();
            if line_trimmed.is_empty() {
                continue;
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

            let mut chars = line_trimmed.chars().peekable();
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
    /// Validate a script name rather than silently rewriting it.
    ///
    /// The previous code *stripped* the illegal characters, so two distinct names could
    /// collapse onto one wire name and one script's `DELETESCRIPT`/`GETSCRIPT`/`PUTSCRIPT`
    /// would silently operate on the other — concretely, `sanitize_name("my{1}")` equalled
    /// `sanitize_name("my1")`. A name of only illegal characters also produced
    /// `PUTSCRIPT "" {n+}`, which RFC 5804 rejects.
    fn validate_name(name: &str) -> Result<&str, String> {
        if name.is_empty() {
            return Err("Sieve script name must not be empty".into());
        }
        if let Some(bad) = name
            .chars()
            .find(|c| matches!(c, '\r' | '\n' | '"' | '\\' | '/' | ';' | '{' | '}'))
        {
            return Err(format!(
                "Sieve script name contains illegal character {bad:?}"
            ));
        }
        Ok(name)
    }

    pub fn put_script(name: &str, content: &str) -> Result<String, String> {
        let clean = Self::validate_name(name)?;
        let bytes = content.len();
        Ok(format!(
            "PUTSCRIPT \"{clean}\" {{{bytes}+}}\r\n{content}\r\n"
        ))
    }

    pub fn get_script(name: &str) -> Result<String, String> {
        let clean = Self::validate_name(name)?;
        Ok(format!("GETSCRIPT \"{clean}\"\r\n"))
    }

    pub fn set_active(name: &str) -> Result<String, String> {
        let clean = Self::validate_name(name)?;
        Ok(format!("SETACTIVE \"{clean}\"\r\n"))
    }

    pub fn list_scripts() -> &'static str {
        "LISTSCRIPTS\r\n"
    }

    pub fn delete_script(name: &str) -> Result<String, String> {
        let clean = Self::validate_name(name)?;
        Ok(format!("DELETESCRIPT \"{clean}\"\r\n"))
    }

    pub fn check_space(name: &str, size_bytes: usize) -> Result<String, String> {
        let clean = Self::validate_name(name)?;
        Ok(format!("HAVESPACE \"{clean}\" {size_bytes}\r\n"))
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
        let cmd = ManageSieveCommand::put_script("default", script).unwrap();
        assert_eq!(cmd, "PUTSCRIPT \"default\" {5+}\r\nkeep;\r\n");
        assert_eq!(
            ManageSieveCommand::set_active("default").unwrap(),
            "SETACTIVE \"default\"\r\n"
        );
        assert_eq!(ManageSieveCommand::list_scripts(), "LISTSCRIPTS\r\n");
    }

    /// Regression: names were *silently stripped* of illegal characters, so `my{1}` and
    /// `my1` collapsed onto the same wire name and a DELETE/PUT hit the wrong script.
    #[test]
    fn managesieve_rejects_ambiguous_script_names() {
        for bad in ["my{1}", "a\"b", "a\\b", "a/b", "a;b", "a\nb", ""] {
            assert!(
                ManageSieveCommand::delete_script(bad).is_err(),
                "illegal script name {bad:?} was accepted"
            );
            assert!(ManageSieveCommand::put_script(bad, "keep;").is_err());
        }
        assert!(ManageSieveCommand::get_script("my1").is_ok());
        assert!(ManageSieveCommand::check_space("my1", 10).is_ok());
    }

    /// Regression: a `#` that did not start the line was not recognised as a comment, so a
    /// trailing comment's `}` was counted — rejecting valid scripts and accepting invalid
    /// ones.
    #[test]
    fn sieve_validator_ignores_trailing_comments() {
        // Valid: the `}` in the comment must not be counted, and the real `}` closes the block.
        let valid = concat!(
            "require [\"fileinto\"];\n",
            "if header :contains \"subject\" \"hi\" {\n",
            "    fileinto \"Junk\";   # move it }\n",
            "}\n",
        );
        assert!(
            SieveValidator::validate(valid).is_ok(),
            "{:?}",
            SieveValidator::validate(valid)
        );

        // Invalid: the only `}` is inside a comment, so the block is never closed.
        let invalid = concat!(
            "require [\"fileinto\"];\n",
            "if header :contains \"subject\" \"hi\" {   # }\n",
        );
        assert!(SieveValidator::validate(invalid).is_err());

        // A `#` inside a string is data, not a comment.
        let str_hash = concat!(
            "require [\"fileinto\"];\n",
            "if header :contains \"subject\" \"a#b\" {\n",
            "    fileinto \"Junk\";\n",
            "}\n",
        );
        assert!(SieveValidator::validate(str_hash).is_ok());
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
