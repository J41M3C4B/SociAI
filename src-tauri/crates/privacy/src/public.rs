//! Documents that are public by nature: the calls of the funders.

use super::{RegexScanner, ScanReport, SensitiveScanner, Severity};

/// The scanner for a document of the funder, not of the institution. A call is public: the telephone, the
/// e-mail and the names of whoever it tells to write to are part of what it says and the reading must
/// keep them. What is never kept is a person's identity data (CURP, personal RFC, voter key, bank
/// account, card, social security): those findings are covered, here and before the model sees a page.
pub struct PublicDocScanner(RegexScanner);

impl PublicDocScanner {
    pub fn new(inner: RegexScanner) -> Self {
        PublicDocScanner(inner)
    }

    /// The whole scanner, for what is checked with every rule (a list of people is rejected whole).
    pub fn whole(&self) -> &RegexScanner {
        &self.0
    }
}

impl SensitiveScanner for PublicDocScanner {
    fn scan(&self, text: &str) -> ScanReport {
        let mut r = self.0.scan(text);
        r.findings.retain(|f| f.severity == Severity::Block);
        r
    }

    fn redact(&self, text: &str, report: &ScanReport) -> String {
        self.0.redact(text, report)
    }
}
