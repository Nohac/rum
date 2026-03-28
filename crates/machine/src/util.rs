use crate::error::RumError;

/// Parse a human-readable size string into bytes.
///
/// Accepts formats like `"20G"`, `"512M"`, `"100K"`, `"1073741824"`.
/// Uses binary units (1G = 1024³ = 1,073,741,824 bytes).
pub fn parse_size(s: &str) -> Result<u64, RumError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(RumError::Validation {
            message: "size cannot be empty".into(),
        });
    }

    // Split into numeric part and suffix
    let (num_str, suffix) = match s.find(|c: char| c.is_ascii_alphabetic()) {
        Some(i) => (&s[..i], s[i..].to_ascii_uppercase()),
        None => (s, String::new()),
    };

    let num: u64 = num_str.parse().map_err(|_| RumError::Validation {
        message: format!("invalid size number: '{num_str}'"),
    })?;

    let multiplier: u64 = match suffix.as_str() {
        "" => 1,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        "T" | "TB" => 1024 * 1024 * 1024 * 1024,
        _ => {
            return Err(RumError::Validation {
                message: format!("unknown size suffix: '{suffix}' (use G, M, K, or T)"),
            });
        }
    };

    num.checked_mul(multiplier)
        .ok_or_else(|| RumError::Validation {
            message: format!("size overflows: '{s}'"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_gibibytes() {
        assert_eq!(parse_size("20G").unwrap(), 20 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_mebibytes() {
        assert_eq!(parse_size("512M").unwrap(), 512 * 1024 * 1024);
    }

    #[test]
    fn parse_size_kibibytes() {
        assert_eq!(parse_size("100K").unwrap(), 100 * 1024);
    }

    #[test]
    fn parse_size_bytes() {
        assert_eq!(parse_size("1073741824").unwrap(), 1073741824);
    }

    #[test]
    fn parse_size_rejects_empty() {
        assert!(parse_size("").is_err());
    }

    #[test]
    fn parse_size_rejects_bad_suffix() {
        assert!(parse_size("10X").is_err());
    }
}
