pub struct SmimeEngine;

impl SmimeEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn verify(&self, _cms_data: &[u8]) -> anyhow::Result<bool> {
        // Real: use x509-cert + cms crates to validate chain
        Ok(false)
    }

    pub fn decrypt(&self, _cms_data: &[u8], _private_key: &[u8]) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("S/MIME decrypt stub")
    }
}

impl Default for SmimeEngine {
    fn default() -> Self {
        Self::new()
    }
}
