//! Icon values are backend strings rendered by the webview as `<img src>`.
//!
//! Uploads are rasterised to `data:image/png` or `data:image/webp`, so a
//! `data:` image and a relative name are the only values an icon may hold.
//! Everything else — `http(s)`, `file`, `blob`, protocol-relative `//host` and
//! other scheme-like strings — would make the webview contact another origin,
//! so it is rejected on write and withheld on read.

use crate::VaultError;

/// Whether an icon value may be rendered.
pub fn is_allowed_icon(value: &str) -> bool {
    is_allowed_trimmed(value.trim())
}

/// The rule behind `is_allowed_icon`, for callers that already trimmed.
///
/// `data:` must be an image. A relative value must not carry a scheme, be
/// protocol-relative, or contain whitespace, control characters or
/// backslashes that a URL parser could reinterpret as a host.
fn is_allowed_trimmed(value: &str) -> bool {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return false;
    }

    let lower = value.to_ascii_lowercase();
    if lower.starts_with("data:") {
        return is_raster_data_url(&lower);
    }

    if value.starts_with("//") {
        return false;
    }

    !has_scheme(value)
}

/// Validate and normalise an icon submitted to a write command. Empty means
/// "not set"; a disallowed value is a validation error rather than a silent
/// drop, so the caller learns its value was refused.
pub fn validate_icon(value: Option<String>) -> Result<Option<String>, VaultError> {
    let Some(value) = value else {
        return Ok(None);
    };

    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    if is_allowed_trimmed(value) {
        Ok(Some(value.to_string()))
    } else {
        Err(VaultError::Validation(
            "Icon must be a data: image or a relative name".to_string(),
        ))
    }
}

/// Withhold an icon value the renderer must not request. Used on read so a
/// tampered or imported vault cannot reach the webview even when it bypassed
/// write-time validation.
pub fn safe_icon(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    is_allowed_trimmed(value).then(|| value.to_string())
}

/// The upload transform emits PNG and WebP only, so those are the only
/// `data:` images an icon may carry.
fn is_raster_data_url(lower: &str) -> bool {
    ["data:image/png", "data:image/webp"].iter().any(|prefix| {
        lower
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(';') || rest.starts_with(','))
    })
}

/// Whether the value begins with `scheme:` (RFC 3986), which would make the
/// webview treat it as an absolute URL.
fn has_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };

    let mut scheme = value[..colon].chars();
    scheme.next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allows_data_images_and_relative_names() {
        assert!(is_allowed_icon(
            "data:image/png;base64,iVBORw0KGgo="
        ));
        assert!(is_allowed_icon("data:image/webp;base64,UklGRg=="));
        assert!(is_allowed_icon("github"));
        assert!(is_allowed_icon("/icons/acme.png"));
    }

    #[test]
    fn test_rejects_values_that_would_fetch_another_origin() {
        for value in [
            "",
            "   ",
            "https://attacker.example/x.png",
            "http://attacker.example/x.png",
            "HTTPS://attacker.example/x.png",
            "file:///C:/secret.png",
            "blob:https://attacker.example/123",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=",
            "data:image/jpeg;base64,/9j/4AAQ",
            "//attacker.example/x.png",
            "\\\\attacker.example\\x.png",
            "/\\attacker.example/x.png",
            "C:\\icons\\x.png",
            "java\tscript:alert(1)",
        ] {
            assert!(!is_allowed_icon(value), "must reject {value:?}");
        }
    }

    #[test]
    fn test_validate_icon_trims_and_treats_empty_as_unset() {
        assert_eq!(validate_icon(None).unwrap(), None);
        assert_eq!(validate_icon(Some("   ".to_string())).unwrap(), None);
        assert_eq!(
            validate_icon(Some("  data:image/png;base64,abc  ".to_string())).unwrap(),
            Some("data:image/png;base64,abc".to_string())
        );
    }

    #[test]
    fn test_validate_icon_rejects_a_remote_url() {
        let result = validate_icon(Some("https://attacker.example/x.png".to_string()));

        assert!(
            matches!(result, Err(VaultError::Validation(_))),
            "got {result:?}"
        );
    }

    #[test]
    fn test_safe_icon_withholds_a_disallowed_value() {
        assert_eq!(safe_icon(Some("github")), Some("github".to_string()));
        assert_eq!(safe_icon(Some("https://attacker.example/x.png")), None);
        assert_eq!(safe_icon(None), None);
    }
}
