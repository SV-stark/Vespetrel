//! URL Tracker Cleaner & Anti-Phishing Link Analyzer §7 Phase 5
use ahash::AHashSet;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::LazyLock;

static TRACKING_SET: LazyLock<AHashSet<&'static str>> =
    LazyLock::new(|| TRACKING_PARAMS.iter().copied().collect());

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
    /// URL targets a raw IP address (e.g., `http://192.168.1.1/login`, or hex `0x7f000001`, or decimal IP)
    RawIpAddress {
        ip: String,
    },
    /// Internationalized Domain Name (IDN) or mixed-script homograph spoofing
    PunycodeHomograph {
        domain: String,
    },
    /// Embedded userinfo spoofing destination (e.g. `http://paypal.com@evil.com/`)
    UserInfoSpoofing {
        user_info: String,
        target_domain: String,
    },
}

/// Strip common analytics and privacy-violating tracker query parameters from a URL,
/// including matrix parameters, fragment trackers, percent-encoded keys, and unwrapping redirectors.
pub fn clean_tracking_url(raw_url: &str) -> String {
    let unwrap_redirect = |url: &str| -> Option<String> {
        if let Ok(parsed) = url::Url::parse(url) {
            let host = parsed.host_str().unwrap_or_default();
            if host.ends_with("google.com") && parsed.path() == "/url" {
                for (k, v) in parsed.query_pairs() {
                    if k == "q" || k == "url" {
                        return Some(v.into_owned());
                    }
                }
            } else if host.ends_with("safelinks.protection.outlook.com") {
                for (k, v) in parsed.query_pairs() {
                    if k == "url" {
                        return Some(v.into_owned());
                    }
                }
            }
        }
        None
    };

    let target_url = unwrap_redirect(raw_url).unwrap_or_else(|| raw_url.to_string());

    let bytes = target_url.as_bytes();
    let qmark_pos = memchr::memchr(b'?', bytes);
    let hash_pos = memchr::memchr(b'#', bytes);

    let (base_and_matrix, query_and_frag) = match (qmark_pos, hash_pos) {
        (Some(qp), Some(hp)) if qp < hp => (
            &target_url[..qp],
            Some((&target_url[qp + 1..hp], Some(&target_url[hp + 1..]))),
        ),
        (Some(qp), None) => (&target_url[..qp], Some((&target_url[qp + 1..], None))),
        (None, Some(hp)) => (&target_url[..hp], Some(("", Some(&target_url[hp + 1..])))),
        (Some(qp), Some(hp)) => (
            &target_url[..qp],
            Some((&target_url[qp + 1..], Some(&target_url[hp + 1..]))),
        ),
        (None, None) => (&target_url[..], None),
    };

    // Clean matrix parameters in path (e.g. /path;utm_source=foo)
    let cleaned_base = if base_and_matrix.contains(';') {
        let parts: Vec<&str> = base_and_matrix.split(';').collect();
        let mut base_res = parts[0].to_string();
        for &param in &parts[1..] {
            let key = match memchr::memchr(b'=', param.as_bytes()) {
                Some(eq) => &param[..eq],
                None => param,
            };
            let decoded_key = urlencoding::decode(key).unwrap_or_else(|_| key.into());
            if !TRACKING_SET
                .iter()
                .any(|&track| track.eq_ignore_ascii_case(decoded_key.as_ref()))
            {
                base_res.push(';');
                base_res.push_str(param);
            }
        }
        base_res
    } else {
        base_and_matrix.to_string()
    };

    let (query_str, frag_str) = match query_and_frag {
        Some((q, f)) => (q, f),
        None => return cleaned_base,
    };

    let cleaned_query_pairs: Vec<&str> = if !query_str.is_empty() {
        query_str
            .split('&')
            .filter(|pair| {
                if pair.is_empty() {
                    return false;
                }
                let key = match memchr::memchr(b'=', pair.as_bytes()) {
                    Some(eq_pos) => &pair[..eq_pos],
                    None => *pair,
                };
                let decoded_key = urlencoding::decode(key).unwrap_or_else(|_| key.into());
                !TRACKING_SET
                    .iter()
                    .any(|&track| track.eq_ignore_ascii_case(decoded_key.as_ref()))
            })
            .collect()
    } else {
        Vec::new()
    };

    let cleaned_frag_pairs: Vec<&str> = if let Some(frag) = frag_str {
        if frag.contains('&') || frag.contains('=') {
            frag.split('&')
                .filter(|pair| {
                    let key = match memchr::memchr(b'=', pair.as_bytes()) {
                        Some(eq_pos) => &pair[..eq_pos],
                        None => *pair,
                    };
                    let decoded_key = urlencoding::decode(key).unwrap_or_else(|_| key.into());
                    !TRACKING_SET
                        .iter()
                        .any(|&track| track.eq_ignore_ascii_case(decoded_key.as_ref()))
                })
                .collect()
        } else {
            vec![frag]
        }
    } else {
        Vec::new()
    };

    let mut result = cleaned_base;
    if !cleaned_query_pairs.is_empty() {
        result.push('?');
        result.push_str(&cleaned_query_pairs.join("&"));
    }

    if frag_str.is_some() {
        result.push('#');
        result.push_str(&cleaned_frag_pairs.join("&"));
    }

    result
}

/// Analyze a link for deceptive phishing indicators
pub fn analyze_phishing_risk(href: &str, display_text: &str) -> PhishingRisk {
    let href_clean = href.trim();
    let text_clean = display_text.trim();

    if href_clean.is_empty() || text_clean.is_empty() {
        return PhishingRisk::Safe;
    }

    let href_lower = href_clean.to_lowercase();
    let text_lower = text_clean.to_lowercase();

    // Check for userinfo spoofing (e.g. http://paypal.com@evil.com)
    let url_without_scheme = if let Some(s) = href_lower.strip_prefix("https://") {
        s
    } else if let Some(s) = href_lower.strip_prefix("http://") {
        s
    } else {
        &href_lower
    };

    let authority = url_without_scheme
        .split(&['/', '?', '#'][..])
        .next()
        .unwrap_or("");
    if let Some((user_info, host_part)) = authority.split_once('@')
        && !user_info.is_empty()
    {
        let actual_host = host_part.split(':').next().unwrap_or(host_part);
        return PhishingRisk::UserInfoSpoofing {
            user_info: user_info.to_string(),
            target_domain: actual_host.to_string(),
        };
    }

    // Check for host indicators
    if let Some(host) = extract_host(&href_lower) {
        let clean_host = host.trim_matches('[').trim_matches(']');

        // 1. Raw IP address target (IPv4 or IPv6)
        if IpAddr::from_str(clean_host).is_ok() {
            return PhishingRisk::RawIpAddress {
                ip: host.to_string(),
            };
        }

        // Check for integer / hex / octal IP address representations
        if clean_host.chars().all(|c| c.is_ascii_digit()) && clean_host.parse::<u64>().is_ok() {
            return PhishingRisk::RawIpAddress {
                ip: host.to_string(),
            };
        }
        if (clean_host.starts_with("0x") || clean_host.starts_with("0X"))
            && u64::from_str_radix(
                clean_host.trim_start_matches("0x").trim_start_matches("0X"),
                16,
            )
            .is_ok()
        {
            return PhishingRisk::RawIpAddress {
                ip: host.to_string(),
            };
        }

        // 2. Punycode homograph or non-ASCII Cyrillic / mixed scripts
        if host.split('.').any(|part| part.starts_with("xn--"))
            || host.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
        {
            return PhishingRisk::PunycodeHomograph {
                domain: host.to_string(),
            };
        }

        // 3. Check for deceptive display domain
        let looks_like_url = text_lower.starts_with("http://")
            || text_lower.starts_with("https://")
            || (text_lower.contains('.') && !text_lower.contains(' '));

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
    let host = if let Some((_, h)) = without_path.split_once('@') {
        h
    } else {
        without_path
    };

    if host.is_empty() { None } else { Some(host) }
}

fn domains_match(d1: &str, d2: &str) -> bool {
    let clean1 = d1.trim_start_matches("www.").to_ascii_lowercase();
    let clean2 = d2.trim_start_matches("www.").to_ascii_lowercase();
    if clean1 == clean2 {
        return true;
    }
    // Display text may be subdomain of target (e.g. login.example.com -> example.com)
    if clean1.ends_with(&format!(".{clean2}")) {
        return true;
    }
    // Target being subdomain of display is only valid if exactly 1 level (e.g. mail.google.com -> google.com)
    if clean2.ends_with(&format!(".{clean1}")) {
        let prefix = &clean2[..clean2.len() - clean1.len() - 1];
        if !prefix.contains('.') {
            return true;
        }
    }
    false
}

/// Scan HTML or text content for any links containing phishing risks
pub fn scan_content_for_phishing(content: &str) -> Option<PhishingRisk> {
    // Check <a> tags with href attribute
    let mut cursor = content;
    while let Some(a_start) = cursor.find("<a ") {
        let after_a = &cursor[a_start + 3..];
        if let Some(tag_end) = after_a.find('>') {
            let tag_attrs = &after_a[..tag_end];
            let after_tag = &after_a[tag_end + 1..];

            // Find href
            let href_val = if let Some(href_pos) = tag_attrs.find("href=") {
                let href_part = &tag_attrs[href_pos + 5..];
                let quote = href_part.chars().next().unwrap_or(' ');
                if quote == '"' || quote == '\'' {
                    let inside = &href_part[1..];
                    inside.split(quote).next().unwrap_or("")
                } else {
                    href_part.split_whitespace().next().unwrap_or("")
                }
            } else {
                ""
            };

            // Find display text up to </a>
            let display_text = if let Some(close_pos) = after_tag.find("</a>") {
                &after_tag[..close_pos]
            } else {
                ""
            };

            if !href_val.is_empty() {
                let risk = analyze_phishing_risk(href_val, display_text);
                if risk != PhishingRisk::Safe {
                    return Some(risk);
                }
            }

            cursor = after_tag;
        } else {
            break;
        }
    }

    // Also scan standalone URLs in plain text or content
    for word in content.split_whitespace() {
        let clean = word.trim_matches(|c: char| {
            c == '('
                || c == ')'
                || c == '<'
                || c == '>'
                || c == '"'
                || c == '\''
                || c == '.'
                || c == ','
        });
        if clean.starts_with("http://") || clean.starts_with("https://") {
            let risk = analyze_phishing_risk(clean, clean);
            if risk != PhishingRisk::Safe {
                return Some(risk);
            }
        }
    }

    None
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
