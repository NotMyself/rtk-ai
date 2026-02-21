use crate::binlog::{FailedTest, TestSummary};
use lazy_static::lazy_static;
use regex::Regex;
use std::path::{Path, PathBuf};

lazy_static! {
    // Note: (?s) enables DOTALL mode so . matches newlines
    static ref TRX_COUNTERS_RE: Regex = Regex::new(
        r#"<Counters\b(?P<attrs>[^>]*)/?>"#
    )
    .expect("valid regex");
    static ref TRX_TEST_RESULT_RE: Regex = Regex::new(
        r#"(?s)<UnitTestResult\b(?P<attrs>[^>]*)>(?P<body>.*?)</UnitTestResult>"#
    )
    .expect("valid regex");
    static ref TRX_ERROR_MESSAGE_RE: Regex = Regex::new(
        r#"(?s)<ErrorInfo>.*?<Message>(?P<message>.*?)</Message>.*?<StackTrace>(?P<stack>.*?)</StackTrace>.*?</ErrorInfo>"#
    )
    .expect("valid regex");
    static ref TRX_ATTR_RE: Regex =
        Regex::new(r#"(?P<key>[A-Za-z_:][A-Za-z0-9_.:-]*)="(?P<value>[^"]*)""#)
            .expect("valid regex");
}

fn extract_attr_value<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    for captures in TRX_ATTR_RE.captures_iter(attrs) {
        if captures.name("key").map(|m| m.as_str()) != Some(key) {
            continue;
        }

        if let Some(value) = captures.name("value") {
            return Some(value.as_str());
        }
    }

    None
}

fn parse_usize_attr(attrs: &str, key: &str) -> usize {
    extract_attr_value(attrs, key)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
}

/// Parse TRX (Visual Studio Test Results) file to extract test summary.
/// Returns None if the file doesn't exist or isn't a valid TRX file.
pub fn parse_trx_file(path: &Path) -> Option<TestSummary> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_trx_content(&content)
}

pub fn find_recent_trx_in_testresults() -> Option<PathBuf> {
    find_recent_trx_in_dir(Path::new("./TestResults"))
}

fn find_recent_trx_in_dir(dir: &Path) -> Option<PathBuf> {
    if !dir.exists() {
        return None;
    }

    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let is_trx = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("trx"));
            if !is_trx {
                return None;
            }

            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn parse_trx_content(content: &str) -> Option<TestSummary> {
    // Quick check if this looks like a TRX file
    if !content.contains("<TestRun") || !content.contains("</TestRun>") {
        return None;
    }

    let mut summary = TestSummary::default();

    // Extract counters from ResultSummary
    if let Some(captures) = TRX_COUNTERS_RE.captures(content) {
        let attrs = captures.name("attrs").map(|m| m.as_str()).unwrap_or("");
        summary.total = parse_usize_attr(attrs, "total");
        summary.passed = parse_usize_attr(attrs, "passed");
        summary.failed = parse_usize_attr(attrs, "failed");
    }

    // Extract failed tests with details
    for captures in TRX_TEST_RESULT_RE.captures_iter(content) {
        let attrs = captures.name("attrs").map(|m| m.as_str()).unwrap_or("");
        let outcome = extract_attr_value(attrs, "outcome").unwrap_or("Unknown");

        if outcome != "Failed" {
            continue;
        }

        let name = extract_attr_value(attrs, "testName")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let full_match = captures.name("body").map(|m| m.as_str()).unwrap_or("");
        let mut details = Vec::new();

        // Try to extract error message and stack trace
        if let Some(error_caps) = TRX_ERROR_MESSAGE_RE.captures(full_match) {
            if let Some(msg) = error_caps.name("message") {
                details.push(msg.as_str().trim().to_string());
            }
            if let Some(stack) = error_caps.name("stack") {
                // Include first few lines of stack trace
                let stack_lines: Vec<&str> = stack.as_str().lines().take(3).collect();
                if !stack_lines.is_empty() {
                    details.push(stack_lines.join("\n"));
                }
            }
        }

        summary.failed_tests.push(FailedTest { name, details });
    }

    // Calculate skipped from counters if available
    if summary.total > 0 {
        summary.skipped = summary
            .total
            .saturating_sub(summary.passed + summary.failed);
    }

    // Set project count to at least 1 if there were any tests
    if summary.total > 0 {
        summary.project_count = 1;
    }

    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_parse_trx_content_extracts_passed_counts() {
        let trx = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
  <ResultSummary outcome="Completed">
    <Counters total="42" executed="42" passed="40" failed="2" error="0" timeout="0" aborted="0" inconclusive="0" />
  </ResultSummary>
</TestRun>"#;

        let summary = parse_trx_content(trx).expect("valid TRX");
        assert_eq!(summary.total, 42);
        assert_eq!(summary.passed, 40);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_trx_content_extracts_failed_tests_with_details() {
        let trx = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun>
  <Results>
    <UnitTestResult testName="MyTests.Calculator.Add_ShouldFail" outcome="Failed">
      <Output>
        <ErrorInfo>
          <Message>Expected: 5, Actual: 4</Message>
          <StackTrace>at MyTests.Calculator.Add_ShouldFail()\nat line 42</StackTrace>
        </ErrorInfo>
      </Output>
    </UnitTestResult>
  </Results>
  <ResultSummary><Counters total="1" executed="1" passed="0" failed="1" /></ResultSummary>
</TestRun>"#;

        let summary = parse_trx_content(trx).expect("valid TRX");
        assert_eq!(summary.failed_tests.len(), 1);
        assert_eq!(
            summary.failed_tests[0].name,
            "MyTests.Calculator.Add_ShouldFail"
        );
        assert!(summary.failed_tests[0].details[0].contains("Expected: 5, Actual: 4"));
    }

    #[test]
    fn test_parse_trx_content_extracts_counters_when_attribute_order_varies() {
        let trx = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun>
  <ResultSummary outcome="Completed">
    <Counters failed="3" passed="7" executed="10" total="10" />
  </ResultSummary>
</TestRun>"#;

        let summary = parse_trx_content(trx).expect("valid TRX");
        assert_eq!(summary.total, 10);
        assert_eq!(summary.passed, 7);
        assert_eq!(summary.failed, 3);
    }

    #[test]
    fn test_parse_trx_content_extracts_failed_tests_when_attribute_order_varies() {
        let trx = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun>
  <Results>
    <UnitTestResult outcome="Failed" testName="MyTests.Ordering.ShouldStillParse">
      <Output>
        <ErrorInfo>
          <Message>Boom</Message>
          <StackTrace>at MyTests.Ordering.ShouldStillParse()</StackTrace>
        </ErrorInfo>
      </Output>
    </UnitTestResult>
  </Results>
  <ResultSummary><Counters failed="1" passed="0" executed="1" total="1" /></ResultSummary>
</TestRun>"#;

        let summary = parse_trx_content(trx).expect("valid TRX");
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failed_tests.len(), 1);
        assert_eq!(
            summary.failed_tests[0].name,
            "MyTests.Ordering.ShouldStillParse"
        );
    }

    #[test]
    fn test_parse_trx_content_returns_none_for_invalid_xml() {
        let not_trx = "This is not a TRX file";
        assert!(parse_trx_content(not_trx).is_none());
    }

    #[test]
    fn test_find_recent_trx_in_dir_returns_none_when_missing() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let missing_dir = temp_dir.path().join("TestResults");

        let found = find_recent_trx_in_dir(&missing_dir);
        assert!(found.is_none());
    }

    #[test]
    fn test_find_recent_trx_in_dir_picks_newest_trx() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let testresults_dir = temp_dir.path().join("TestResults");
        std::fs::create_dir_all(&testresults_dir).expect("create TestResults");

        let old_trx = testresults_dir.join("old.trx");
        let new_trx = testresults_dir.join("new.trx");
        std::fs::write(&old_trx, "old").expect("write old");
        std::thread::sleep(Duration::from_millis(5));
        std::fs::write(&new_trx, "new").expect("write new");

        let found = find_recent_trx_in_dir(&testresults_dir).expect("should find newest trx");
        assert_eq!(found, new_trx);
    }

    #[test]
    fn test_find_recent_trx_in_dir_ignores_non_trx_files() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let testresults_dir = temp_dir.path().join("TestResults");
        std::fs::create_dir_all(&testresults_dir).expect("create TestResults");

        let txt = testresults_dir.join("notes.txt");
        std::fs::write(&txt, "noop").expect("write txt");

        let found = find_recent_trx_in_dir(&testresults_dir);
        assert!(found.is_none());
    }
}
