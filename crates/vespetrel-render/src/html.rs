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
        .add_generic_attributes(["data-blocked-src"]);

    // Enforce allowlist - ammonia does this by default (removes script etc.)
    builder.clean(html).to_string()
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
        assert!(!out.contains("src=\"https://tracker"));
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
}
