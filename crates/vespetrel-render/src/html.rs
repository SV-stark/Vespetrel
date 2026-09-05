use ammonia::Builder;
use lol_html::{HtmlRewriter, Settings, element};

#[derive(Debug, Clone)]
pub struct RewriteOptions {
    /// Block external images (rewrite src -> data-blocked-src)
    pub block_remote_images: bool,
    /// Rewrite cid: URIs to blob://
    pub rewrite_cid: bool,
}

impl Default for RewriteOptions {
    fn default() -> Self {
        Self {
            block_remote_images: true,
            rewrite_cid: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SanitizeOptions {
    pub rewrite: RewriteOptions,
}

/// Full pipeline: lol_html streaming rewriter -> ammonia sanitizer §5.2
pub fn sanitize(html: &str, opts: &SanitizeOptions) -> anyhow::Result<String> {
    let rewritten = rewrite_html(html, &opts.rewrite)?;
    let cleaned = ammonia_clean(&rewritten);
    Ok(cleaned)
}

const MAX_HTML_INPUT_BYTES: usize = 10 * 1024 * 1024; // 10MB safety limit

fn is_remote_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("//")
}

fn is_tracking_pixel(el: &lol_html::html_content::Element) -> bool {
    let width = el.get_attribute("width").unwrap_or_default().to_lowercase();
    let height = el
        .get_attribute("height")
        .unwrap_or_default()
        .to_lowercase();
    let style = el.get_attribute("style").unwrap_or_default().to_lowercase();

    let is_zero_or_one = |s: &str| s == "0" || s == "1" || s == "0px" || s == "1px";

    if is_zero_or_one(&width) || is_zero_or_one(&height) {
        return true;
    }

    if style.contains("width: 1px")
        || style.contains("width:1px")
        || style.contains("width: 0px")
        || style.contains("width:0px")
        || style.contains("display: none")
        || style.contains("display:none")
        || style.contains("visibility: hidden")
    {
        return true;
    }

    false
}

fn rewrite_html(input: &str, opts: &RewriteOptions) -> anyhow::Result<String> {
    if input.len() > MAX_HTML_INPUT_BYTES {
        anyhow::bail!("HTML input exceeds maximum allowable size");
    }

    let mut output = Vec::with_capacity(input.len());

    let settings = Settings::new()
        .append_element_content_handler(element!(
            "script, iframe, object, embed, applet, base, form, input, button, select, textarea, link, meta",
            |el| {
                el.remove();
                Ok(())
            }
        ))
        .append_element_content_handler(element!("img, source, video, audio, track", |el| {
            // Remove tracking pixels
            if is_tracking_pixel(el) {
                el.remove();
                return Ok(());
            }
            if let Some(src) = el.get_attribute("src") {
                if src.starts_with("cid:") && opts.rewrite_cid {
                    let cid = src.trim_start_matches("cid:");
                    el.set_attribute("src", &format!("blob://{cid}"))?;
                } else if is_remote_url(&src) && opts.block_remote_images {
                    el.set_attribute("data-blocked-src", &src)?;
                    el.remove_attribute("src");
                }
            }
            if let Some(srcset) = el.get_attribute("srcset")
                && opts.block_remote_images
                && (srcset.contains("http://") || srcset.contains("https://") || srcset.contains("//"))
            {
                el.set_attribute("data-blocked-srcset", &srcset)?;
                el.remove_attribute("srcset");
            }
            if let Some(poster) = el.get_attribute("poster")
                && opts.block_remote_images
                && is_remote_url(&poster)
            {
                el.set_attribute("data-blocked-poster", &poster)?;
                el.remove_attribute("poster");
            }

            Ok(())
        }))
        .append_element_content_handler(element!("a", |el| {
            el.set_attribute("rel", "noopener noreferrer")?;
            el.set_attribute("target", "_blank")?;
            if let Some(href) = el.get_attribute("href") {
                let cleaned_href = crate::cleaner::clean_tracking_url(&href);
                if cleaned_href != href {
                    el.set_attribute("href", &cleaned_href)?;
                }
                let risk = crate::cleaner::analyze_phishing_risk(&cleaned_href, &cleaned_href);
                match risk {
                    crate::cleaner::PhishingRisk::Safe => {}
                    crate::cleaner::PhishingRisk::DeceptiveDisplayDomain { .. }
                    | crate::cleaner::PhishingRisk::RawIpAddress { .. }
                    | crate::cleaner::PhishingRisk::PunycodeHomograph { .. }
                    | crate::cleaner::PhishingRisk::UserInfoSpoofing { .. } => {
                        el.set_attribute("data-phishing-risk", "flagged")?;
                        el.set_attribute("title", "Warning: Suspicious destination link")?;
                    }
                }
            }
            Ok(())
        }));

    let mut rewriter = HtmlRewriter::new(settings, |c: &[u8]| output.extend_from_slice(c));

    rewriter
        .write(input.as_bytes())
        .map_err(|e| anyhow::anyhow!("lol_html: {e}"))?;
    rewriter
        .end()
        .map_err(|e| anyhow::anyhow!("lol_html end: {e}"))?;

    let s = simdutf8::basic::from_utf8(&output)
        .map_err(|e| anyhow::anyhow!("UTF-8 validation error: {e}"))?;
    Ok(s.to_string())
}

static CSS_DANGEROUS_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r#"(?i)(@import\s+[^;]+;?|expression\s*\([^)]*\)|javascript\s*:[^;]+|url\s*\(\s*['"]?(?:https?:|//|data:)[^'")]*['"]?\s*\))"#)
        .expect("valid regex")
});

fn ammonia_clean(html: &str) -> String {
    let mut builder = Builder::default();
    builder
        .add_tags(["table", "thead", "tbody", "tr", "th", "td"])
        .link_rel(Some("noopener noreferrer"))
        .add_generic_attributes([
            "data-blocked-src",
            "data-blocked-srcset",
            "data-blocked-poster",
            "data-phishing-risk",
        ])
        .add_url_schemes(["blob", "cid"]);

    let cleaned = builder.clean(html).to_string();
    CSS_DANGEROUS_REGEX
        .replace_all(&cleaned, "/* blocked */")
        .to_string()
}

/// Wrap sanitized email HTML into a full sandboxed HTML document with Content Security Policy (CSP)
pub fn render_sandboxed_document(clean_body: &str, dark_mode: bool) -> String {
    let bg_color = if dark_mode { "#18181b" } else { "#ffffff" };
    let text_color = if dark_mode { "#f4f4f5" } else { "#18181b" };
    let link_color = if dark_mode { "#60a5fa" } else { "#2563eb" };

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src blob: data: cid:; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'; frame-src 'none'; object-src 'none'; script-src 'none';">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body {{
            background-color: {bg_color};
            color: {text_color};
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
            font-size: 14px;
            line-height: 1.6;
            margin: 0;
            padding: 16px;
            word-wrap: break-word;
        }}
        a {{ color: {link_color}; text-decoration: underline; }}
        img {{ max-width: 100%; height: auto; }}
        table {{ max-width: 100%; border-collapse: collapse; }}
    </style>
</head>
<body>
    {clean_body}
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script() {
        let html = r#"<p>Hello</p><script>alert(1)</script>"#;
        let out = sanitize(html, &SanitizeOptions::default()).unwrap();
        assert!(!out.contains("<script"));
        assert!(out.contains("Hello"));
    }

    #[test]
    fn blocks_remote_image() {
        let html = r#"<img src="https://tracker.example.com/pixel.png" width="100" height="100">"#;
        let opts = SanitizeOptions {
            rewrite: RewriteOptions {
                block_remote_images: true,
                rewrite_cid: false,
            },
        };
        let out = sanitize(html, &opts).unwrap();
        assert!(out.contains("data-blocked-src"));
        // Ensure original src attribute is removed (not just renamed) - check for ` src="https://` with leading space
        assert!(!out.contains(" src=\"https://tracker"));
        // Also ensure data-blocked-src is present and holds the original URL
        assert!(out.contains("https://tracker.example.com/pixel.png"));
    }

    #[test]
    fn rewrites_cid() {
        let html = r#"<img src="cid:image001.png@01D">"#;
        let opts = SanitizeOptions {
            rewrite: RewriteOptions {
                block_remote_images: false,
                rewrite_cid: true,
            },
        };
        let out = sanitize(html, &opts).unwrap();
        assert!(out.contains("blob://image001.png@01D"));
    }

    #[test]
    fn removes_tracking_pixel() {
        let html = r#"<img src="https://example.com/pixel.gif" width="1" height="1">"#;
        let out = sanitize(html, &SanitizeOptions::default()).unwrap();
        assert!(!out.contains("pixel.gif"));
    }

    #[test]
    fn generates_sandboxed_document_with_csp() {
        let doc = render_sandboxed_document("<p>Clean Content</p>", true);
        assert!(doc.contains("Content-Security-Policy"));
        assert!(doc.contains("default-src 'none'"));
        assert!(doc.contains("Clean Content"));
        assert!(doc.contains("#18181b")); // Dark mode background
    }
}
