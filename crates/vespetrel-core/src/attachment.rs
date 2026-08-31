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
    pub fn new(
        message_id: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        size_bytes: i64,
        blob_path: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            message_id: message_id.into(),
            content_id: None,
            filename: filename.into(),
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
}
