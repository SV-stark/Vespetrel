use ammonia::Builder;
use lol_html::{HtmlRewriter, Settings, element};

#[derive(Debug, Clone, Default)]
pub struct RewriteOptions {
    /// Block external images (rewrite src -> data-blocked-src)
    pub block_remote_images: bool,
    /// Rewrite cid: URIs to blob://
    pub rewrite_cid: bool,
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
            "script, iframe, object, embed, applet, base",
            |el| {
                el.remove();
                Ok(())
            }
        ))
        .append_element_content_handler(element!("img", |el| {
            // Remove tracking pixels
            if is_tracking_pixel(el) {
                el.remove();
                return Ok(());
            }
            if let Some(src) = el.get_attribute("src") {
                if src.starts_with("cid:") && opts.rewrite_cid {
                    let cid = src.trim_start_matches("cid:");
                    el.set_attribute("src", &format!("blob://{cid}"))?;
                } else if (src.starts_with("http://") || src.starts_with("https://"))
                    && opts.block_remote_images
                {
                    el.set_attribute("data-blocked-src", &src)?;
                    el.remove_attribute("src");
                }
            }
            Ok(())
        }))
        .append_element_content_handler(element!("a", |el| {
            el.set_attribute("rel", "noopener noreferrer")?;
            el.set_attribute("target", "_blank")?;
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

fn ammonia_clean(html: &str) -> String {
    let mut builder = Builder::default();
    builder
        .add_tags([
            "video", "audio", "source", "table", "thead", "tbody", "tr", "th", "td",
        ])
        .link_rel(Some("noopener noreferrer"))
        .add_generic_attributes(["data-blocked-src"])
        .add_url_schemes(["blob", "cid"]);

    builder.clean(html).to_string()
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
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src 'self' blob: data: https:; style-src 'unsafe-inline'; font-src 'self' data:;">
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
