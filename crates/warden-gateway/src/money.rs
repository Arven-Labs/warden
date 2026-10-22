//! Reading and showing money without floats: USD text ⇄ micro-USD.

use crate::store::Micros;

/// `"12"`, `"12.5"` or `"0.000001"` → micro-USD. At most six decimals, no sign, no exponent: anything else is refused
/// instead of being rounded, because this is how an operator types a top-up.
pub fn parse_usd(text: &str) -> Result<Micros, String> {
    let bad = || format!("`{text}` isn't an amount in USD (digits with up to 6 decimals, like 25 or 12.50)");
    let (whole, frac) = match text.split_once('.') {
        Some((w, f)) => (w, f),
        None => (text, ""),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(frac) || frac.len() > 6 {
        return Err(bad());
    }
    let whole: i64 = whole.parse().map_err(|_| bad())?;
    let frac: i64 = format!("{frac:0<6}").parse().map_err(|_| bad())?;
    whole.checked_mul(1_000_000).and_then(|w| w.checked_add(frac)).ok_or_else(bad)
}

/// micro-USD → `$12.345678`.
pub fn format_usd(micros: Micros) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let abs = micros.unsigned_abs();
    format!("{sign}${}.{:06}", abs / 1_000_000, abs % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_parse_exactly() {
        assert_eq!(parse_usd("25"), Ok(25_000_000));
        assert_eq!(parse_usd("12.5"), Ok(12_500_000));
        assert_eq!(parse_usd("0.000001"), Ok(1));
        assert_eq!(parse_usd("0"), Ok(0));
        assert_eq!(parse_usd("7."), Ok(7_000_000));
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in ["", ".5", "-1", "1e3", "1.1234567", "abc", "1,5", " 1", "$1", "99999999999999999999"] {
            assert!(parse_usd(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn amounts_show_with_six_decimals() {
        assert_eq!(format_usd(12_345_678), "$12.345678");
        assert_eq!(format_usd(0), "$0.000000");
        assert_eq!(format_usd(-1), "-$0.000001");
    }

    #[test]
    fn what_is_shown_parses_back() {
        for n in [0, 1, 999_999, 1_000_000, 123_456_789] {
            assert_eq!(parse_usd(format_usd(n).trim_start_matches('$')), Ok(n));
        }
    }
}
