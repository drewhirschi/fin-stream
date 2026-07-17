use url::Url;

use super::{MediaError, MediaResult};

const MEDIA_PREFIX: &str = "/media/loan-workspace/";
const STATIC_PREFIX: &str = "/static/loan-images/";
const CANONICAL_PHOTO_PREFIX: &str = "loan-workspace/";
const MAX_KEY_BYTES: usize = 1_024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PhotoLocation {
    Stored(String),
    ExternalOnly,
}

pub fn classify_photo(image_url: &str) -> MediaResult<PhotoLocation> {
    if image_url != image_url.trim() || image_url.is_empty() {
        return Err(MediaError::InvalidKey);
    }
    if let Some(suffix) = image_url.strip_prefix(MEDIA_PREFIX) {
        return legacy_photo_key(suffix).map(PhotoLocation::Stored);
    }
    if let Some(suffix) = image_url.strip_prefix(STATIC_PREFIX) {
        return legacy_photo_key(suffix).map(PhotoLocation::Stored);
    }

    let parsed = Url::parse(image_url).map_err(|_| MediaError::InvalidKey)?;
    match parsed.scheme() {
        "http" | "https" => {
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(MediaError::InvalidKey);
            }
            let raw_path = raw_http_path(image_url).ok_or(MediaError::InvalidKey)?;
            if let Some(suffix) = raw_path.strip_prefix(MEDIA_PREFIX) {
                if parsed.query().is_some() || parsed.fragment().is_some() {
                    return Err(MediaError::InvalidKey);
                }
                return legacy_photo_key(suffix).map(PhotoLocation::Stored);
            }
            if let Some(suffix) = raw_path.strip_prefix(STATIC_PREFIX) {
                if parsed.query().is_some() || parsed.fragment().is_some() {
                    return Err(MediaError::InvalidKey);
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
                return Err(MediaError::InvalidKey);
            }
            let raw_path = image_url
                .split_once("://")
                .and_then(|(_, remainder)| remainder.find('/').map(|index| &remainder[index + 1..]))
                .ok_or(MediaError::InvalidKey)?;
            canonical_key(&decode_url_key(raw_path)?).map(PhotoLocation::Stored)
        }
        _ => Err(MediaError::InvalidKey),
    }
}

pub(crate) fn canonical_key(raw: &str) -> MediaResult<String> {
    if raw.is_empty() || raw.len() > MAX_KEY_BYTES {
        return Err(MediaError::InvalidKey);
    }
    if raw != raw.trim() || raw.starts_with('/') || raw.ends_with('/') || raw.contains('%') {
        return Err(MediaError::InvalidKey);
    }
    if raw.contains('\\')
        || raw.contains("//")
        || raw.contains(['?', '#'])
        || raw.chars().any(char::is_control)
    {
        return Err(MediaError::InvalidKey);
    }
    if raw
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(MediaError::InvalidKey);
    }
    Ok(raw.to_owned())
}

pub(crate) fn photo_key_from_route(route_key: &str) -> MediaResult<String> {
    canonical_key(&format!("{CANONICAL_PHOTO_PREFIX}{route_key}"))
}

pub fn photo_route_url(canonical_photo_key: &str) -> MediaResult<String> {
    let key = canonical_key(canonical_photo_key)?;
    let suffix = key
        .strip_prefix(CANONICAL_PHOTO_PREFIX)
        .filter(|suffix| !suffix.is_empty())
        .ok_or(MediaError::InvalidKey)?;
    Ok(format!("{MEDIA_PREFIX}{}", aws_encode_path(suffix)))
}

pub fn safe_external_url(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    Some(parsed.into())
}

pub(crate) fn encode_route_segment(raw: &str) -> String {
    aws_encode(raw.as_bytes(), false)
}

pub(crate) fn aws_encode_path(raw: &str) -> String {
    aws_encode(raw.as_bytes(), true)
}

pub(crate) fn aws_encode_query(raw: &str) -> String {
    aws_encode(raw.as_bytes(), false)
}

fn legacy_photo_key(raw_suffix: &str) -> MediaResult<String> {
    if raw_suffix.contains(['?', '#']) {
        return Err(MediaError::InvalidKey);
    }
    let suffix = decode_url_key(raw_suffix)?;
    canonical_key(&format!("{CANONICAL_PHOTO_PREFIX}{suffix}"))
}

fn raw_http_path(value: &str) -> Option<&str> {
    let (_, remainder) = value.split_once("://")?;
    remainder
        .find('/')
        .map_or(Some("/"), |index| Some(&remainder[index..]))
}

fn known_s3_http_key(host: &str, raw_path: &str) -> MediaResult<Option<String>> {
    let host = host.to_ascii_lowercase();
    let raw_path = raw_path
        .split(['?', '#'])
        .next()
        .ok_or(MediaError::InvalidKey)?;
    let path = raw_path.strip_prefix('/').ok_or(MediaError::InvalidKey)?;

    let raw_key = if is_aws_virtual_host(&host) {
        if path.is_empty() {
            return Err(MediaError::InvalidKey);
        }
        Some(path)
    } else if is_aws_path_host(&host) || host.ends_with(".r2.cloudflarestorage.com") {
        Some(
            path.split_once('/')
                .map(|(_, key)| key)
                .filter(|key| !key.is_empty())
                .ok_or(MediaError::InvalidKey)?,
        )
    } else {
        None
    };

    raw_key
        .map(decode_url_key)
        .transpose()?
        .map(|key| canonical_key(&key))
        .transpose()
}

fn is_aws_virtual_host(host: &str) -> bool {
    let Some((bucket, suffix)) = host.split_once(".s3") else {
        return false;
    };
    !bucket.is_empty()
        && (suffix == ".amazonaws.com"
            || (suffix.starts_with('.') && suffix.ends_with(".amazonaws.com"))
            || (suffix.starts_with('-') && suffix.ends_with(".amazonaws.com")))
}

fn is_aws_path_host(host: &str) -> bool {
    host == "s3.amazonaws.com"
        || (host.starts_with("s3.") && host.ends_with(".amazonaws.com"))
        || (host.starts_with("s3-") && host.ends_with(".amazonaws.com"))
}

fn decode_url_key(raw: &str) -> MediaResult<String> {
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
            return Err(MediaError::InvalidKey);
        }
        let high = decode_hex(bytes[cursor + 1]).ok_or(MediaError::InvalidKey)?;
        let low = decode_hex(bytes[cursor + 2]).ok_or(MediaError::InvalidKey)?;
        let value = high * 16 + low;
        if matches!(value, b'/' | b'\\' | b'.' | b'%' | 0..=31 | 127) {
            return Err(MediaError::InvalidKey);
        }
        decoded.push(value);
        cursor += 3;
    }
    String::from_utf8(decoded).map_err(|_| MediaError::InvalidKey)
}

fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn aws_encode(bytes: &[u8], preserve_slashes: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(bytes.len());
    for byte in bytes {
        if byte.is_ascii_alphanumeric()
            || matches!(*byte, b'-' | b'_' | b'.' | b'~')
            || (preserve_slashes && *byte == b'/')
        {
            encoded.push(char::from(*byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}
