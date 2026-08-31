use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub message_id: String,
    pub content_id: Option<String>,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub blob_path: String,
    pub is_inline: bool,
}

impl Attachment {
    pub fn sanitize_filename(raw: &str) -> String {
        let clean = raw.replace(['\0', '/', '\\'], "_");
        let name = std::path::Path::new(&clean)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("attachment")
            .trim();
        if name.is_empty() || name == ".." || name == "." {
            "attachment".to_string()
        } else {
            name.to_string()
        }
    }

    pub fn new(
        message_id: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        size_bytes: i64,
        blob_path: impl Into<String>,
    ) -> Self {
        let safe_filename = Self::sanitize_filename(&filename.into());
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            message_id: message_id.into(),
            content_id: None,
            filename: safe_filename,
            content_type: content_type.into(),
            size_bytes,
            blob_path: blob_path.into(),
            is_inline: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attachment_creation() {
        let att = Attachment::new(
            "msg_1",
            "invoice.pdf",
            "application/pdf",
            1024,
            "blobs/inv.lz4",
        );
        assert_eq!(att.filename, "invoice.pdf");
        assert_eq!(att.size_bytes, 1024);
        assert!(!att.is_inline);
    }

    #[test]
    fn test_attachment_filename_sanitization() {
        let att = Attachment::new(
            "msg_1",
            "../../etc/passwd\0evil.exe",
            "application/octet-stream",
            50,
            "blobs/1.lz4",
        );
        assert!(!att.filename.contains('/'));
        assert!(!att.filename.contains('\\'));
        assert!(!att.filename.contains('\0'));
    }
}
