//! Boot report: what was loaded, skipped and failed during plugin side-loading.
//!
//! Hard rule (ADR 0009 §4): side-load failures are NEVER fatal — they surface
//! here and boot continues.

/// Aggregated side-load outcomes. Surfaced by `NuomiKernel::boot_report()`
/// and `tracing::info!` after boot.
#[derive(Debug, Default, Clone)]
pub struct BootReport {
    pub loaded: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub failed: Vec<(String, String)>,
}

impl BootReport {
    pub fn record_loaded(&mut self, id: impl Into<String>) {
        self.loaded.push(id.into());
    }

    /// `reason` is free text, e.g. "duplicate of 'upper'" or "no plugin.toml".
    pub fn record_skipped(&mut self, dir: impl Into<String>, reason: impl Into<String>) {
        self.skipped.push((dir.into(), reason.into()));
    }

    /// `reason` is the manifest/spawn/handshake error string.
    pub fn record_failed(&mut self, dir: impl Into<String>, reason: impl Into<String>) {
        self.failed.push((dir.into(), reason.into()));
    }

    pub fn log_summary(&self) {
        tracing::info!(
            loaded = self.loaded.len(),
            skipped = self.skipped.len(),
            failed = self.failed.len(),
            "plugin side-load report: loaded {:?}, skipped {:?}, failed {:?}",
            self.loaded,
            self.skipped,
            self.failed
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_all_outcomes() {
        let mut report = BootReport::default();
        report.record_loaded("upper");
        report.record_skipped("dir-b", "duplicate of 'upper'");
        report.record_failed("dir-c", "manifest unreadable");
        assert_eq!(report.loaded, vec!["upper".to_string()]);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.failed.len(), 1);
        report.log_summary(); // smoke: must not panic
    }
}
