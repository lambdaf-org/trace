//! Opinionated, deterministic verdicts. Each one prints the metric that
//! triggered it — the verdict is only as good as the tally behind it.

use crate::model::{DaySummary, Verdict};

fn hm(ms: i64) -> String {
    let m = ms / 60_000;
    format!("{}h{:02}m", m / 60, m % 60)
}

fn cat_ms(s: &DaySummary, cat: &str) -> i64 {
    s.categories
        .iter()
        .find(|c| c.category == cat)
        .map(|c| c.ms)
        .unwrap_or(0)
}

pub fn evaluate(s: &DaySummary) -> Vec<Verdict> {
    let build_ms = s.build_ms;
    let mut out = Vec::new();
    let ratio_pct = s.focus_ratio * 100.0;
    let active_h = s.active_ms as f64 / 3_600_000.0;

    // Real build day.
    if active_h >= 5.0 && s.focus_ratio >= 0.60 {
        out.push(Verdict {
            line: "This was a real build day.".into(),
            evidence: format!(
                "{:.1}% of active time in build tools ({} of {})",
                ratio_pct,
                hm(build_ms),
                hm(s.active_ms)
            ),
        });
    }

    // Prompting vs implementing.
    let ai = cat_ms(s, "ai");
    let code = cat_ms(s, "code");
    if ai > 0 && ai > code {
        out.push(Verdict {
            line: "You prompted more than you implemented.".into(),
            evidence: format!("AI tools {} vs code {}", hm(ai), hm(code)),
        });
    }

    // Fragmented, not lazy.
    let mean_active_ms = {
        let active_segments = s.top_apps.len().max(1) as i64;
        s.active_ms / active_segments
    };
    if s.context_switches >= 80 && mean_active_ms < 3 * 60_000 {
        out.push(Verdict {
            line: "Your day was not lazy. It was fragmented.".into(),
            evidence: format!(
                "{} context switches, longest focus only {}",
                s.context_switches,
                hm(s.longest_focus_ms)
            ),
        });
    }

    // Browser leak.
    let browser = cat_ms(s, "browser");
    let top_non_build = s
        .categories
        .iter()
        .find(|c| c.category != "code" && c.category != "terminal")
        .map(|c| (c.category.clone(), c.ms));
    if let Some((cat, ms)) = top_non_build {
        if cat == "browser" && browser == ms && s.focus_ratio < 0.50 {
            out.push(Verdict {
                line: "Your biggest leak was the browser.".into(),
                evidence: format!(
                    "{} in browser; build never recovered past {:.0}%",
                    hm(browser),
                    ratio_pct
                ),
            });
        }
    }

    // Claim vs reality.
    if let Some(label) = &s.label {
        let claims_build = label.to_lowercase().contains("cod")
            || label.to_lowercase().contains("build")
            || label.to_lowercase().contains("ship");
        if claims_build && s.focus_ratio < 0.50 {
            out.push(Verdict {
                line: format!("You called this a {}.", label),
                evidence: format!("Only {:.0}% of it was inside build tools.", ratio_pct),
            });
        }
    }

    if out.is_empty() {
        out.push(Verdict {
            line: "No verdict — not enough signal yet.".into(),
            evidence: format!(
                "{} active, {} switches",
                hm(s.active_ms),
                s.context_switches
            ),
        });
    }
    out
}
