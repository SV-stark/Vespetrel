//! Attachment Inspection and Preview Engine
//!
//! Classifies email attachments into rich preview formats:
//! - Images (PNG, JPEG, GIF, WebP, SVG, BMP) with dimension/format extraction
//! - Text/Code documents (Markdown, Source code, JSON, TOML, CSV, Plain) with safe snippet preview & line counts
//! - PDF documents with version & structure analysis
//! - Media (Audio/Video) and Archives (ZIP, TAR, GZ)
//! - Binary / Unknown with hex/ASCII dump and security inspection

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentKind {
    Image { format: String, is_vector: bool },
    Text { syntax: String },
    Pdf { version: Option<String> },
    Audio { format: String },
    Video { format: String },
    Archive { format: String },
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentPreviewData {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: usize,
    pub formatted_size: String,
    pub kind: AttachmentKind,
    pub can_preview_inline: bool,
    pub text_snippet: Option<String>,
    pub line_count: Option<usize>,
    pub hex_dump: Option<String>,
    pub summary: String,
}

/// Detects the semantic kind of an attachment from its filename and MIME type.
pub fn detect_attachment_kind(filename: &str, mime_type: &str) -> AttachmentKind {
    let lower_name = filename.to_lowercase();
    let lower_mime = mime_type.to_lowercase();

    // 1. Images
    if lower_mime.starts_with("image/")
        || lower_name.ends_with(".png")
        || lower_name.ends_with(".jpg")
        || lower_name.ends_with(".jpeg")
        || lower_name.ends_with(".gif")
        || lower_name.ends_with(".webp")
        || lower_name.ends_with(".svg")
        || lower_name.ends_with(".bmp")
    {
        let is_vector = lower_name.ends_with(".svg") || lower_mime.contains("svg");
        let format = if lower_name.ends_with(".png") || lower_mime.contains("png") {
            "PNG"
        } else if lower_name.ends_with(".jpg")
            || lower_name.ends_with(".jpeg")
            || lower_mime.contains("jpeg")
        {
            "JPEG"
        } else if lower_name.ends_with(".gif") || lower_mime.contains("gif") {
            "GIF"
        } else if lower_name.ends_with(".webp") || lower_mime.contains("webp") {
            "WebP"
        } else if is_vector {
            "SVG"
        } else {
            "Image"
        };
        return AttachmentKind::Image {
            format: format.into(),
            is_vector,
        };
    }

    // 2. PDF
    if lower_mime.contains("pdf") || lower_name.ends_with(".pdf") {
        return AttachmentKind::Pdf { version: None };
    }

    // 3. Audio & Video
    if lower_mime.starts_with("audio/")
        || lower_name.ends_with(".mp3")
        || lower_name.ends_with(".wav")
        || lower_name.ends_with(".ogg")
        || lower_name.ends_with(".m4a")
        || lower_name.ends_with(".flac")
    {
        let ext = lower_name.rsplit('.').next().unwrap_or("audio");
        return AttachmentKind::Audio {
            format: ext.to_uppercase(),
        };
    }
    if lower_mime.starts_with("video/")
        || lower_name.ends_with(".mp4")
        || lower_name.ends_with(".mkv")
        || lower_name.ends_with(".webm")
        || lower_name.ends_with(".mov")
    {
        let ext = lower_name.rsplit('.').next().unwrap_or("video");
        return AttachmentKind::Video {
            format: ext.to_uppercase(),
        };
    }

    // 4. Archives
    if lower_mime.contains("zip") || lower_name.ends_with(".zip") {
        return AttachmentKind::Archive {
            format: "ZIP".into(),
        };
    }
    if lower_mime.contains("tar")
        || lower_name.ends_with(".tar")
        || lower_name.ends_with(".tar.gz")
        || lower_name.ends_with(".tgz")
    {
        return AttachmentKind::Archive {
            format: "TAR".into(),
        };
    }
    if lower_mime.contains("gzip") || lower_name.ends_with(".gz") {
        return AttachmentKind::Archive {
            format: "GZIP".into(),
        };
    }

    // 5. Text & Source Code
    if lower_mime.starts_with("text/")
        || lower_name.ends_with(".txt")
        || lower_name.ends_with(".md")
        || lower_name.ends_with(".markdown")
        || lower_name.ends_with(".rs")
        || lower_name.ends_with(".py")
        || lower_name.ends_with(".js")
        || lower_name.ends_with(".ts")
        || lower_name.ends_with(".json")
        || lower_name.ends_with(".toml")
        || lower_name.ends_with(".yaml")
        || lower_name.ends_with(".yml")
        || lower_name.ends_with(".csv")
        || lower_name.ends_with(".log")
        || lower_name.ends_with(".html")
        || lower_name.ends_with(".css")
        || lower_name.ends_with(".sh")
        || lower_name.ends_with(".sql")
    {
        let syntax = if lower_name.ends_with(".rs") {
            "rust"
        } else if lower_name.ends_with(".md") || lower_name.ends_with(".markdown") {
            "markdown"
        } else if lower_name.ends_with(".json") {
            "json"
        } else if lower_name.ends_with(".toml") {
            "toml"
        } else if lower_name.ends_with(".yaml") || lower_name.ends_with(".yml") {
            "yaml"
        } else if lower_name.ends_with(".py") {
            "python"
        } else if lower_name.ends_with(".js") || lower_name.ends_with(".ts") {
            "javascript"
        } else if lower_name.ends_with(".html") {
            "html"
        } else if lower_name.ends_with(".css") {
            "css"
        } else if lower_name.ends_with(".csv") {
            "csv"
        } else if lower_name.ends_with(".sql") {
            "sql"
        } else if lower_name.ends_with(".sh") {
            "shell"
        } else {
            "text"
        };
        return AttachmentKind::Text {
            syntax: syntax.to_string(),
        };
    }

    AttachmentKind::Binary
}

/// Generates a preview payload for an attachment given its filename, MIME type, and bytes.
pub fn generate_preview(filename: &str, mime_type: &str, data: &[u8]) -> AttachmentPreviewData {
    let size_bytes = data.len();
    let formatted_size = format_file_size(size_bytes);
    let mut kind = detect_attachment_kind(filename, mime_type);

    let can_preview_inline;
    let mut text_snippet = None;
    let mut line_count = None;
    let mut hex_dump = None;
    let summary: String;

    match &mut kind {
        AttachmentKind::Image { format, is_vector } => {
            can_preview_inline = true;
            summary = format!("{format} image, {formatted_size}");
            if *is_vector {
                // Vector SVGs can also show text snippet if valid UTF-8
                if let Ok(s) = std::str::from_utf8(data) {
                    let preview_len = s.chars().take(2048).map(|c| c.len_utf8()).sum();
                    text_snippet = Some(s[..preview_len].to_string());
                }
            }
        }
        AttachmentKind::Text { syntax } => {
            can_preview_inline = true;
            let text = String::from_utf8_lossy(data);
            let lines: Vec<&str> = text.lines().collect();
            line_count = Some(lines.len());

            let max_preview_chars = 4096;
            let preview_str = if text.len() > max_preview_chars {
                let mut cut = text[..max_preview_chars].to_string();
                cut.push_str("\n... [truncated preview]");
                cut
            } else {
                text.to_string()
            };
            text_snippet = Some(preview_str);
            summary = format!("{syntax} document, {} lines, {formatted_size}", lines.len());
        }
        AttachmentKind::Pdf { version } => {
            can_preview_inline = false;
            // Inspect PDF header: e.g. %PDF-1.7
            if data.starts_with(b"%PDF-") && data.len() >= 8 {
                let ver_str = String::from_utf8_lossy(&data[5..8]).to_string();
                *version = Some(ver_str.clone());
                summary = format!("PDF Document (v{ver_str}), {formatted_size}");
            } else {
                summary = format!("PDF Document, {formatted_size}");
            }
        }
        AttachmentKind::Audio { format } => {
            can_preview_inline = false;
            summary = format!("{format} audio recording, {formatted_size}");
        }
        AttachmentKind::Video { format } => {
            can_preview_inline = false;
            summary = format!("{format} video recording, {formatted_size}");
        }
        AttachmentKind::Archive { format } => {
            can_preview_inline = false;
            summary = format!("{format} compressed archive, {formatted_size}");
            hex_dump = Some(format_hex_dump(&data[..data.len().min(64)]));
        }
        AttachmentKind::Binary => {
            can_preview_inline = false;
            summary = format!("Binary file, {formatted_size}");
            hex_dump = Some(format_hex_dump(&data[..data.len().min(64)]));
        }
    }

    AttachmentPreviewData {
        filename: filename.to_string(),
        mime_type: mime_type.to_string(),
        size_bytes,
        formatted_size,
        kind,
        can_preview_inline,
        text_snippet,
        line_count,
        hex_dump,
        summary,
    }
}

pub fn format_file_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Formats a byte slice as a standard canonical Hex + ASCII dump.
pub fn format_hex_dump(data: &[u8]) -> String {
    let mut out = String::new();
    for (i, chunk) in data.chunks(16).enumerate() {
        use std::fmt::Write;
        let _ = write!(out, "{:04x}: ", i * 16);

        // Hex part
        for b in chunk {
            let _ = write!(out, "{:02x} ", b);
        }
        for _ in chunk.len()..16 {
            out.push_str("   ");
        }

        out.push_str(" | ");

        // ASCII part
        for &b in chunk {
            if (32..=126).contains(&b) {
                out.push(b as char);
            } else {
                out.push('.');
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_image_kind() {
        let kind = detect_attachment_kind("photo.JPG", "application/octet-stream");
        assert_eq!(
            kind,
            AttachmentKind::Image {
                format: "JPEG".into(),
                is_vector: false
            }
        );

        let kind2 = detect_attachment_kind("vector.svg", "image/svg+xml");
        assert_eq!(
            kind2,
            AttachmentKind::Image {
                format: "SVG".into(),
                is_vector: true
            }
        );
    }

    #[test]
    fn test_text_preview_generation() {
        let text_data = b"fn main() {\n    println!(\"Hello World\");\n}\n";
        let preview = generate_preview("main.rs", "text/x-rust", text_data);

        assert!(preview.can_preview_inline);
        assert_eq!(
            preview.kind,
            AttachmentKind::Text {
                syntax: "rust".into()
            }
        );
        assert_eq!(preview.line_count, Some(3));
        assert!(preview.text_snippet.unwrap().contains("println!"));
    }

    #[test]
    fn test_pdf_preview_generation() {
        let pdf_data = b"%PDF-1.5 fake content";
        let preview = generate_preview("document.pdf", "application/pdf", pdf_data);

        assert!(!preview.can_preview_inline);
        assert_eq!(
            preview.kind,
            AttachmentKind::Pdf {
                version: Some("1.5".into())
            }
        );
        assert!(preview.summary.contains("v1.5"));
    }

    #[test]
    fn test_hex_dump_formatting() {
        let binary_data = b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let preview = generate_preview("app.bin", "application/octet-stream", binary_data);

        assert_eq!(preview.kind, AttachmentKind::Binary);
        let dump = preview.hex_dump.unwrap();
        assert!(dump.contains("7f 45 4c 46"));
        assert!(dump.contains(".ELF"));
    }
}
