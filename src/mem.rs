//! Process memory sampling for `phg bench`. std-only and Linux-only: it reads
//! `/proc/self/status` (`VmRSS` = current resident set, `VmHWM` = peak resident set) and resets the
//! kernel's peak high-water mark through `/proc/self/clear_refs`. Every function returns an
//! `Option` (or is a silent no-op) when `/proc` is unavailable — non-Linux hosts, sandboxes, the
//! cross-built Windows/macOS binaries — so `bench` reports "memory: unavailable on this platform"
//! rather than failing. No crates, no `unsafe`: just a file read and a file write (EV-7 spirit —
//! a missing or malformed `/proc` entry yields `None`, never a panic).

/// Current resident set size in KiB (`VmRSS` from `/proc/self/status`), or `None` when the file is
/// unreadable or lacks the field (i.e. not running on Linux).
#[must_use]
pub fn current_rss_kb() -> Option<u64> {
    read_status_field("VmRSS:")
}

/// Peak resident set size in KiB (`VmHWM` from `/proc/self/status`), or `None` when unavailable.
/// The kernel tracks this as a monotonic high-water mark; [`reset_peak_rss`] rewinds it so a later
/// phase's peak can be attributed independently of earlier ones.
#[must_use]
pub fn peak_rss_kb() -> Option<u64> {
    read_status_field("VmHWM:")
}

/// Reset the kernel's peak-RSS high-water mark (`VmHWM`) down to the current `VmRSS` by writing `5`
/// to `/proc/self/clear_refs` (supported since Linux 4.0). This lets `bench` measure the peak
/// *growth* a single phase causes rather than the whole process's lifetime peak. A silent no-op
/// when the file is absent or unwritable — the write error is intentionally discarded.
pub fn reset_peak_rss() {
    let _ = std::fs::write("/proc/self/clear_refs", "5\n");
}

/// Parse one `Key:\t<number> kB` line out of `/proc/self/status`, returning the number (KiB).
/// `prefix` includes the trailing colon (e.g. `"VmRSS:"`). `None` if the file or field is missing
/// or the value doesn't parse.
fn read_status_field(prefix: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix(prefix) {
            // `rest` is like "\t   12345 kB" — the first whitespace-delimited token is the number.
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_fields_are_readable_on_linux() {
        if cfg!(target_os = "linux") {
            // A running process always has a non-zero resident set and a recorded peak.
            assert!(current_rss_kb().is_some_and(|kb| kb > 0));
            assert!(peak_rss_kb().is_some_and(|kb| kb > 0));
        }
    }

    #[test]
    fn reset_peak_never_panics() {
        // Must be safe on every platform, including where /proc/self/clear_refs is absent.
        reset_peak_rss();
    }

    #[test]
    fn peak_is_at_least_current() {
        // The kernel guarantees `VmHWM >= VmRSS` only *within a single snapshot* of
        // `/proc/self/status`. `current_rss_kb()` and `peak_rss_kb()` do two separate reads, and
        // cargo runs these tests in parallel threads of one process — a sibling allocating (raising
        // the shared VmRSS) or calling `reset_peak_rss` (lowering the shared VmHWM) between the two
        // reads can make `peak < cur` even though the invariant always holds per-snapshot. So read
        // both fields from ONE snapshot here.
        let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
            return; // non-Linux / no procfs — nothing to assert
        };
        let field = |prefix: &str| -> Option<u64> {
            status.lines().find_map(|l| {
                l.strip_prefix(prefix)
                    .and_then(|r| r.split_whitespace().next())
                    .and_then(|n| n.parse().ok())
            })
        };
        if let (Some(cur), Some(peak)) = (field("VmRSS:"), field("VmHWM:")) {
            assert!(peak >= cur, "VmHWM {peak} < VmRSS {cur}");
        }
    }
}
