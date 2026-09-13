//! Canonical Runtime Order, ported from `PolicyRuntimeOrder.cs`:
//! priority ascending, then creation timestamp ascending, then persisted
//! source order. Creation timestamps are `yyyy-MM-dd HH:mm` strings, which
//! sort correctly lexicographically; `sort_by_key`/`then_` chains below use a
//! stable sort so the persisted order is the final tiebreak.

use crate::rule::ProxyRule;

pub fn canonical_order(rules: Vec<ProxyRule>) -> Vec<ProxyRule> {
    let mut indexed: Vec<(usize, ProxyRule)> = rules.into_iter().enumerate().collect();
    indexed.sort_by(|(ia, a), (ib, b)| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| ia.cmp(ib))
    });
    indexed.into_iter().map(|(_, rule)| rule).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::ProxyMode;

    fn rule(exe: &str, priority: i32, created: &str) -> ProxyRule {
        let mut r = ProxyRule::new(format!("id-{exe}"), exe);
        r.priority = priority;
        r.created_at = created.to_string();
        r.mode = ProxyMode::Direct;
        r
    }

    // Port of PolicyRuntimeOrderTests.MoveRule_UsesTheCanonicalOrderShownByTheUi.
    #[test]
    fn orders_by_priority_then_timestamp_then_source_order() {
        let rules = vec![
            rule("source-first.exe", 30, "2026-01-01 00:00"),
            rule("runtime-first.exe", 10, "2026-01-01 00:00"),
            rule("runtime-second.exe", 20, "2026-01-01 00:00"),
        ];

        let ordered = canonical_order(rules);

        assert_eq!(
            ordered.iter().map(|r| r.exe_name.as_str()).collect::<Vec<_>>(),
            vec!["runtime-first.exe", "runtime-second.exe", "source-first.exe"]
        );
        assert_eq!(
            ordered.iter().map(|r| r.priority).collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn equal_priority_breaks_tie_by_timestamp_then_source_order() {
        let rules = vec![
            rule("late.exe", 10, "2026-02-01 00:00"),
            rule("early.exe", 10, "2026-01-01 00:00"),
            rule("early-2.exe", 10, "2026-01-01 00:00"),
        ];

        let ordered = canonical_order(rules);
        assert_eq!(
            ordered.iter().map(|r| r.exe_name.as_str()).collect::<Vec<_>>(),
            vec!["early.exe", "early-2.exe", "late.exe"]
        );
    }
}
