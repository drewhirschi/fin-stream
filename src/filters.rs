/// Format a float as currency with commas: 1234567.89 → "1,234,567.89"
pub fn currency(value: f64) -> String {
    let is_negative = value < 0.0;
    let abs = value.abs();
    let whole = abs as u64;
    let cents = ((abs - whole as f64) * 100.0).round() as u64;

    let whole_str = format_with_commas(whole);
    let formatted = format!("{whole_str}.{cents:02}");

    if is_negative {
        format!("-{formatted}")
    } else {
        formatted
    }
}

/// Format a float as whole number with commas: 1234567.0 → "1,234,567"
pub fn whole(value: f64) -> String {
    let is_negative = value < 0.0;
    let abs = value.abs().round() as u64;
    let formatted = format_with_commas(abs);

    if is_negative {
        format!("-{formatted}")
    } else {
        formatted
    }
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}
