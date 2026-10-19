// Token/cost usage totals and formatting.
use crate::store::{State, UsageTotals};

/// Sum of usage across all threads.
pub fn grand_total(state: &State) -> UsageTotals {
    let mut total = UsageTotals::default();
    for t in &state.threads {
        total.add(&t.usage);
    }
    total
}

/// Compact token count: 950 -> "950", 1_200 -> "1.2k", 45_600 -> "45.6k",
/// 1_300_000 -> "1.3M". Trailing ".0" is trimmed (1_000 -> "1k").
pub fn fmt_tokens(n: u64) -> String {
    fn trim(s: String, suffix: &str) -> String {
        s.strip_suffix(".0").map(|s| s.to_string()).unwrap_or(s) + suffix
    }
    if n >= 1_000_000 {
        trim(format!("{:.1}", n as f64 / 1_000_000.0), "M")
    } else if n >= 1_000 {
        trim(format!("{:.1}", n as f64 / 1_000.0), "k")
    } else {
        n.to_string()
    }
}

/// Dollar cost from micros: 4 decimals below $1 ("$0.0134"), else 2 ("$1.23").
pub fn fmt_cost(micros: u64) -> String {
    let dollars = micros as f64 / 1_000_000.0;
    if dollars < 1.0 {
        format!("${dollars:.4}")
    } else {
        format!("${dollars:.2}")
    }
}
