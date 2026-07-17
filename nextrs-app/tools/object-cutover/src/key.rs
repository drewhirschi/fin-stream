use url::Url;

const MEDIA_PREFIX: &str = "/media/loan-workspace/";
const STATIC_PREFIX: &str = "/static/loan-images/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhotoLocation {
    Stored(String),
    ExternalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    Empty,
    Absolute,
    Traversal,
    AmbiguousEncoding,
    InvalidCharacter,
    InvalidPhotoLocation,
}

pub fn classify_photo(image_url: &str) -> Result<PhotoLocation, KeyError> {
    if image_url != image_url.trim() || image_url.is_empty() {
        return Err(KeyError::InvalidPhotoLocation);
    }

    if let Some(suffix) = image_url.strip_prefix(MEDIA_PREFIX) {
        return legacy_photo_key(suffix).map(PhotoLocation::Stored);
    }
    if let Some(suffix) = image_url.strip_prefix(STATIC_PREFIX) {
        return legacy_photo_key(suffix).map(PhotoLocation::Stored);
    }

    let parsed = Url::parse(image_url).map_err(|_| KeyError::InvalidPhotoLocation)?;
    match parsed.scheme() {
        "http" | "https" => {
            if parsed.username() != "" || parsed.password().is_some() {
                return Err(KeyError::InvalidPhotoLocation);
            }
            let raw_path = raw_http_path(image_url).ok_or(KeyError::InvalidPhotoLocation)?;
            if let Some(suffix) = raw_path.strip_prefix(MEDIA_PREFIX) {
                if parsed.query().is_some() || parsed.fragment().is_some() {
                    return Err(KeyError::AmbiguousEncoding);
                }
                return legacy_photo_key(suffix).map(PhotoLocation::Stored);
            }
            if let Some(suffix) = raw_path.strip_prefix(STATIC_PREFIX) {
                if parsed.query().is_some() || parsed.fragment().is_some() {
                    return Err(KeyError::AmbiguousEncoding);
                }
                return legacy_photo_key(suffix).map(PhotoLocation::Stored);
            }
            if let Some(key) = known_s3_http_key(parsed.host_str().unwrap_or_default(), raw_path)? {
                return Ok(PhotoLocation::Stored(key));
            }
            Ok(PhotoLocation::ExternalOnly)
        }
        "s3" => {
            if parsed.host_str().is_none()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(KeyError::InvalidPhotoLocation);
            }
            let raw_path = image_url
                .split_once("://")
                .and_then(|(_, remainder)| remainder.find('/').map(|index| &remainder[index + 1..]))
                .ok_or(KeyError::InvalidPhotoLocation)?;
            let key = decode_url_key(raw_path)?;
            canonical_key(&key).map(PhotoLocation::Stored)
        }
        _ => Err(KeyError::InvalidPhotoLocation),
    }
}

fn legacy_photo_key(raw_suffix: &str) -> Result<String, KeyError> {
    if raw_suffix.contains(['?', '#']) {
        return Err(KeyError::AmbiguousEncoding);
    }
    let suffix = decode_url_key(raw_suffix)?;
    canonical_key(&format!("loan-workspace/{suffix}"))
}

fn raw_http_path(value: &str) -> Option<&str> {
    let (_, remainder) = value.split_once("://")?;
    let path_start = remainder.find('/');
    match path_start {
        Some(index) => Some(&remainder[index..]),
        None => Some("/"),
    }
}

fn known_s3_http_key(host: &str, raw_path: &str) -> Result<Option<String>, KeyError> {
    let host = host.to_ascii_lowercase();
    let raw_path = raw_path
        .split(['?', '#'])
        .next()
        .ok_or(KeyError::InvalidPhotoLocation)?;
    let without_leading_slash = raw_path
        .strip_prefix('/')
        .ok_or(KeyError::InvalidPhotoLocation)?;

    let raw_key = if is_aws_virtual_host(&host) {
        if without_leading_slash.is_empty() {
            return Err(KeyError::InvalidPhotoLocation);
        }
        Some(without_leading_slash)
    } else if is_aws_path_host(&host) || host.ends_with(".r2.cloudflarestorage.com") {
        // Path-style endpoints include the bucket as the first path segment.
        Some(
            without_leading_slash
                .split_once('/')
                .map(|(_bucket, key)| key)
                .ok_or(KeyError::InvalidPhotoLocation)?,
        )
    } else {
        None
    };

    let Some(raw_key) = raw_key else {
        return Ok(None);
    };
    let decoded = decode_url_key(raw_key)?;
    canonical_key(&decoded).map(Some)
}

fn is_aws_virtual_host(host: &str) -> bool {
    let Some((bucket, s3_suffix)) = host.split_once(".s3") else {
        return false;
    };
    !bucket.is_empty()
        && (s3_suffix == ".amazonaws.com"
            || (s3_suffix.starts_with('.') && s3_suffix.ends_with(".amazonaws.com"))
            || (s3_suffix.starts_with('-') && s3_suffix.ends_with(".amazonaws.com")))
}

fn is_aws_path_host(host: &str) -> bool {
    host == "s3.amazonaws.com"
        || (host.starts_with("s3.") && host.ends_with(".amazonaws.com"))
        || (host.starts_with("s3-") && host.ends_with(".amazonaws.com"))
}

pub fn canonical_database_key(raw: &str) -> Result<String, KeyError> {
    canonical_key(raw)
}

pub fn canonical_key(raw: &str) -> Result<String, KeyError> {
    validate_key(raw, true)
}

/// Validate a key exactly as it exists in the retained legacy object store.
/// The old attachment writer admitted literal percent signs in filenames; in
/// this context they are S3 key bytes, not URL encoding. New destination keys
/// remain subject to `canonical_key` and therefore reject percent signs.
pub fn legacy_physical_key(raw: &str) -> Result<String, KeyError> {
    validate_key(raw, false)
}

fn validate_key(raw: &str, reject_percent: bool) -> Result<String, KeyError> {
    if raw.is_empty() {
        return Err(KeyError::Empty);
    }
    if raw != raw.trim() || raw.starts_with('/') || raw.ends_with('/') {
        return Err(KeyError::Absolute);
    }
    if reject_percent && raw.contains('%') {
        return Err(KeyError::AmbiguousEncoding);
    }
    if raw.contains('\\') || raw.contains("//") || raw.contains('\0') {
        return Err(KeyError::InvalidCharacter);
    }
    if raw.chars().any(|character| character.is_control()) {
        return Err(KeyError::InvalidCharacter);
    }

    for segment in raw.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(KeyError::Traversal);
        }
    }

    Ok(raw.to_owned())
}

fn decode_url_key(raw: &str) -> Result<String, KeyError> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'%' {
            decoded.push(bytes[cursor]);
            cursor += 1;
            continue;
        }
        if cursor + 2 >= bytes.len() {
            return Err(KeyError::AmbiguousEncoding);
        }
        let high = decode_hex(bytes[cursor + 1]).ok_or(KeyError::AmbiguousEncoding)?;
        let low = decode_hex(bytes[cursor + 2]).ok_or(KeyError::AmbiguousEncoding)?;
        let value = high * 16 + low;
        if matches!(value, b'/' | b'\\' | b'.' | b'%' | 0..=31 | 127) {
            return Err(KeyError::AmbiguousEncoding);
        }
        decoded.push(value);
        cursor += 3;
    }
    String::from_utf8(decoded).map_err(|_| KeyError::AmbiguousEncoding)
}

fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_legacy_prefixes_have_one_canonical_namespace() {
        assert_eq!(
            classify_photo("/media/loan-workspace/loan-1/front.jpg"),
            Ok(PhotoLocation::Stored(
                "loan-workspace/loan-1/front.jpg".to_owned()
            ))
        );
        assert_eq!(
            classify_photo("/static/loan-images/loan-1/front.jpg"),
            Ok(PhotoLocation::Stored(
                "loan-workspace/loan-1/front.jpg".to_owned()
            ))
        );
        assert_eq!(
            classify_photo("https://old.example/media/loan-workspace/loan-1/front.jpg"),
            Ok(PhotoLocation::Stored(
                "loan-workspace/loan-1/front.jpg".to_owned()
            ))
        );
    }

    #[test]
    fn external_photos_are_classified_without_exposing_the_url() {
        assert_eq!(
            classify_photo("https://images.example/listing/front.jpg?width=800"),
            Ok(PhotoLocation::ExternalOnly)
        );
    }

    #[test]
    fn known_s3_http_url_styles_are_stored_objects() {
        for value in [
            "https://bucket.s3.us-west-2.amazonaws.com/loan-workspace/1/front.jpg",
            "https://s3.us-west-2.amazonaws.com/bucket/loan-workspace/1/front.jpg",
            "https://account.r2.cloudflarestorage.com/bucket/loan-workspace/1/front.jpg",
        ] {
            assert_eq!(
                classify_photo(value),
                Ok(PhotoLocation::Stored(
                    "loan-workspace/1/front.jpg".to_owned()
                )),
                "failed to classify {value}"
            );
        }
    }

    #[test]
    fn traversal_and_ambiguous_encodings_are_rejected() {
        for value in [
            "/media/loan-workspace/../secret",
            "/media/loan-workspace/%2e%2e/secret",
            "/media/loan-workspace/%2Fetc/passwd",
            "/media/loan-workspace/a%252Fb",
            "/media/loan-workspace//front.jpg",
            "/media/loan-workspace/front.jpg?token=secret",
        ] {
            assert!(classify_photo(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn legacy_physical_keys_allow_literal_percent_but_canonical_keys_do_not() {
        assert!(canonical_key("emails/one/100%.pdf").is_err());
        assert_eq!(
            legacy_physical_key("emails/one/100%.pdf"),
            Ok("emails/one/100%.pdf".to_owned())
        );
        assert!(legacy_physical_key("emails/../secret").is_err());
    }

    #[test]
    fn safe_space_encoding_is_decoded_once() {
        assert_eq!(
            classify_photo("/media/loan-workspace/loan%201/front.jpg"),
            Ok(PhotoLocation::Stored(
                "loan-workspace/loan 1/front.jpg".to_owned()
            ))
        );
    }

    #[test]
    fn raw_database_keys_must_already_be_canonical() {
        assert!(canonical_database_key("emails/one/body.html").is_ok());
        assert!(canonical_database_key("/emails/one/body.html").is_err());
        assert!(canonical_database_key("emails/../secret").is_err());
        assert!(canonical_database_key("emails/%2e%2e/secret").is_err());
    }
}
