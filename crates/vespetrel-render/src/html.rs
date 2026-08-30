use ammonia::Builder;
use lol_html::{element, HtmlRewriter, Settings};

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

fn rewrite_html(input: &str, opts: &RewriteOptions) -> anyhow::Result<String> {
    let mut output = Vec::new();

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                // Remove script, iframe, object, embed, applet
                element!("script, iframe, object, embed, applet", |el| {
                    el.remove();
                    Ok(())
                }),
                // Handle img: strip 1x1 trackers, block remote src, rewrite cid
                element!("img", |el| {
                    // Tag 1x1 tracking pixels
                    if let Some(width) = el.get_attribute("width") {
                        if let Some(height) = el.get_attribute("height") {
                            if width == "1" && height == "1" {
                                el.remove();
                                return Ok(());
                            }
                        }
                    }
                    if let Some(src) = el.get_attribute("src") {
                        if src.starts_with("cid:") && opts.rewrite_cid {
                            let cid = src.trim_start_matches("cid:");
                            el.set_attribute("src", &format!("blob://{cid}"))?;
                        } else if src.starts_with("http://") || src.starts_with("https://") {
                            if opts.block_remote_images {
                                el.set_attribute("data-blocked-src", &src)?;
                                el.remove_attribute("src");
                            }
                        }
                    }
                    Ok(())
                }),
                element!("a", |el| {
                    el.set_attribute("rel", "noopener noreferrer")?;
                    el.set_attribute("target", "_blank")?;
                    Ok(())
                }),
            ],
            ..Settings::new()
        },
        |c: &[u8]| output.extend_from_slice(c),
    );

    rewriter.write(input.as_bytes()).map_err(|e| anyhow::anyhow!("lol_html: {e}"))?;
    rewriter.end().map_err(|e| anyhow::anyhow!("lol_html end: {e}"))?;

    Ok(String::from_utf8(output)?)
}

fn ammonia_clean(html: &str) -> String {
    let mut builder = Builder::default();
    builder
        .add_tags(["video", "audio", "source", "table", "thead", "tbody", "tr", "th", "td"])
        .link_rel(Some("noopener noreferrer"))
        .add_generic_attributes(["data-blocked-src"])
        .add_url_schemes(["blob", "cid", "data"]);

    // Enforce allowlist - ammonia does this by default (removes script etc.)
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
        let opts = SanitizeOptions { rewrite: RewriteOptions { block_remote_images: true, rewrite_cid: false } };
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
        let opts = SanitizeOptions { rewrite: RewriteOptions { block_remote_images: false, rewrite_cid: true } };
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
