//! URL Tracker Cleaner & Anti-Phishing Link Analyzer §7 Phase 5
use ahash::AHashSet;

const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "utm_id",
    "fbclid",
    "gclid",
    "dclid",
    "msclkid",
    "yclid",
    "igshid",
    "mc_eid",
    "mc_cid",
    "_hsenc",
    "_hsmi",
    "mkt_tok",
    "trk",
    "ref_src",
    "si",
    "gbraid",
    "wbraid",
    "twclid",
    "ttclid",
    "sc_lid",
    "gclsrc",
];

/// Result of anti-phishing inspection on an HTML anchor or link
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhishingRisk {
    Safe,
    /// Display text appears to be a different domain than actual target href (e.g., text: `paypal.com`, href: `evil.com`)
    DeceptiveDisplayDomain {
        display_domain: String,
        target_domain: String,
    },
    /// URL targets a raw IP address (e.g., `http://192.168.1.1/login`)
    RawIpAddress {
        ip: String,
    },
    /// Internationalized Domain Name (IDN) with potential Cyrillic/homograph spoofing
    PunycodeHomograph {
        domain: String,
    },
}

/// Strip common analytics and privacy-violating tracker query parameters from a URL
pub fn clean_tracking_url(raw_url: &str) -> String {
    let bytes = raw_url.as_bytes();
    let qmark_pos = match memchr::memchr(b'?', bytes) {
        Some(pos) => pos,
        None => return raw_url.to_string(),
    };

    let base = &raw_url[..qmark_pos];
    let query_and_fragment = &raw_url[qmark_pos + 1..];

    let (query, fragment) = match memchr::memchr(b'#', query_and_fragment.as_bytes()) {
        Some(hash_pos) => (
            &query_and_fragment[..hash_pos],
            Some(&query_and_fragment[hash_pos + 1..]),
        ),
        None => (query_and_fragment, None),
    };

    let track_set: AHashSet<&str> = TRACKING_PARAMS.iter().copied().collect();

    let cleaned_pairs: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            if pair.is_empty() {
                return false;
            }
            let key = match memchr::memchr(b'=', pair.as_bytes()) {
                Some(eq_pos) => &pair[..eq_pos],
                None => *pair,
            };
            let key_lower = key.to_ascii_lowercase();
            !track_set.contains(key_lower.as_str())
        })
        .collect();

    let mut result = base.to_string();
    if !cleaned_pairs.is_empty() {
        result.push('?');
        result.push_str(&cleaned_pairs.join("&"));
    }

    if let Some(f) = fragment {
        result.push('#');
        result.push_str(f);
    }

    result
}

/// Analyze a link for deceptive phishing indicators
pub fn analyze_phishing_risk(href: &str, display_text: &str) -> PhishingRisk {
    let href_clean = href.trim();
    let text_clean = display_text.trim();

    let href_lower = href_clean.to_lowercase();
    let text_lower = text_clean.to_lowercase();

    // 1. Check for raw IP address target
    if let Some(host) = extract_host(&href_lower) {
        if host.split('.').all(|part| part.parse::<u8>().is_ok()) && host.split('.').count() == 4 {
            return PhishingRisk::RawIpAddress {
                ip: host.to_string(),
            };
        }

        // 2. Check for Punycode homograph
        if host.starts_with("xn--") || host.contains(".xn--") {
            return PhishingRisk::PunycodeHomograph {
                domain: host.to_string(),
            };
        }

        // 3. Check for deceptive display domain
        let looks_like_url = text_lower.starts_with("http://")
            || text_lower.starts_with("https://")
            || text_lower.contains('.') && !text_lower.contains(' ');

        let mismatched_host = if looks_like_url {
            extract_host(&text_lower).filter(|dh| !domains_match(dh, host))
        } else {
            None
        };

        if let Some(display_host) = mismatched_host {
            return PhishingRisk::DeceptiveDisplayDomain {
                display_domain: display_host.to_string(),
                target_domain: host.to_string(),
            };
        }
    }

    PhishingRisk::Safe
}

fn extract_host(url: &str) -> Option<&str> {
    let stripped = if let Some(s) = url.strip_prefix("https://") {
        s
    } else if let Some(s) = url.strip_prefix("http://") {
        s
    } else {
        url
    };

    let without_path = stripped.split(&['/', '?', '#', ':'][..]).next()?;
    if without_path.is_empty() {
        None
    } else {
        Some(without_path)
    }
}

fn domains_match(d1: &str, d2: &str) -> bool {
    let clean1 = d1.trim_start_matches("www.");
    let clean2 = d2.trim_start_matches("www.");
    clean1 == clean2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_tracking_url() {
        let dirty = "https://example.com/item?id=123&utm_source=newsletter&utm_medium=email&fbclid=IwAR0123#section2";
        let cleaned = clean_tracking_url(dirty);
        assert_eq!(cleaned, "https://example.com/item?id=123#section2");

        let no_tracking = "https://example.com/page?article=5";
        assert_eq!(clean_tracking_url(no_tracking), no_tracking);
    }

    #[test]
    fn test_analyze_phishing_deceptive_domain() {
        let risk = analyze_phishing_risk(
            "https://evil-phishing.com/login",
            "https://paypal.com/signin",
        );
        assert!(matches!(risk, PhishingRisk::DeceptiveDisplayDomain { .. }));

        let safe = analyze_phishing_risk("https://paypal.com/signin", "https://paypal.com/signin");
        assert_eq!(safe, PhishingRisk::Safe);
    }

    #[test]
    fn test_analyze_phishing_raw_ip() {
        let risk = analyze_phishing_risk("http://192.168.1.100/account", "Click here to login");
        assert!(matches!(risk, PhishingRisk::RawIpAddress { .. }));
    }

    #[test]
    fn test_analyze_phishing_punycode() {
        let risk = analyze_phishing_risk("https://xn--pple-43d.com", "Apple Support");
        assert!(matches!(risk, PhishingRisk::PunycodeHomograph { .. }));
    }
}
