use crate::binlog::{FailedTest, TestSummary};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::path::{Path, PathBuf};

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|b| *b == b':').next().unwrap_or(name)
}

fn extract_attr_value(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    key: &[u8],
) -> Option<String> {
    for attr in start.attributes().flatten() {
        if local_name(attr.key.as_ref()) != key {
            continue;
        }

        if let Ok(value) = attr.decode_and_unescape_value(reader.decoder()) {
            return Some(value.into_owned());
        }
    }

    None
}

fn parse_usize_attr(reader: &Reader<&[u8]>, start: &BytesStart<'_>, key: &[u8]) -> usize {
    extract_attr_value(reader, start, key)
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
    #[derive(Clone, Copy)]
    enum CaptureField {
        Message,
        StackTrace,
    }

    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut summary = TestSummary::default();
    let mut saw_test_run = false;
    let mut in_failed_result = false;
    let mut in_error_info = false;
    let mut failed_test_name = String::new();
    let mut message_buf = String::new();
    let mut stack_buf = String::new();
    let mut capture_field: Option<CaptureField> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                b"TestRun" => saw_test_run = true,
                b"Counters" => {
                    summary.total = parse_usize_attr(&reader, &e, b"total");
                    summary.passed = parse_usize_attr(&reader, &e, b"passed");
                    summary.failed = parse_usize_attr(&reader, &e, b"failed");
                }
                b"UnitTestResult" => {
                    let outcome = extract_attr_value(&reader, &e, b"outcome")
                        .unwrap_or_else(|| "Unknown".to_string());

                    if outcome == "Failed" {
                        in_failed_result = true;
                        in_error_info = false;
                        capture_field = None;
                        message_buf.clear();
                        stack_buf.clear();
                        failed_test_name = extract_attr_value(&reader, &e, b"testName")
                            .unwrap_or_else(|| "unknown".to_string());
                    }
                }
                b"ErrorInfo" => {
                    if in_failed_result {
                        in_error_info = true;
                    }
                }
                b"Message" => {
                    if in_failed_result && in_error_info {
                        capture_field = Some(CaptureField::Message);
                        message_buf.clear();
                    }
                }
                b"StackTrace" => {
                    if in_failed_result && in_error_info {
                        capture_field = Some(CaptureField::StackTrace);
                        stack_buf.clear();
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match local_name(e.name().as_ref()) {
                b"Counters" => {
                    summary.total = parse_usize_attr(&reader, &e, b"total");
                    summary.passed = parse_usize_attr(&reader, &e, b"passed");
                    summary.failed = parse_usize_attr(&reader, &e, b"failed");
                }
                b"UnitTestResult" => {
                    let outcome = extract_attr_value(&reader, &e, b"outcome")
                        .unwrap_or_else(|| "Unknown".to_string());
                    if outcome == "Failed" {
                        let name = extract_attr_value(&reader, &e, b"testName")
                            .unwrap_or_else(|| "unknown".to_string());
                        summary.failed_tests.push(FailedTest {
                            name,
                            details: Vec::new(),
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Text(e)) => {
                if !in_failed_result {
                    buf.clear();
                    continue;
                }

                let text = String::from_utf8_lossy(e.as_ref());
                match capture_field {
                    Some(CaptureField::Message) => message_buf.push_str(&text),
                    Some(CaptureField::StackTrace) => stack_buf.push_str(&text),
                    None => {}
                }
            }
            Ok(Event::CData(e)) => {
                if !in_failed_result {
                    buf.clear();
                    continue;
                }

                let text = String::from_utf8_lossy(e.as_ref());
                match capture_field {
                    Some(CaptureField::Message) => message_buf.push_str(&text),
                    Some(CaptureField::StackTrace) => stack_buf.push_str(&text),
                    None => {}
                }
            }
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                b"Message" | b"StackTrace" => {
                    capture_field = None;
                }
                b"ErrorInfo" => {
                    in_error_info = false;
                }
                b"UnitTestResult" => {
                    if in_failed_result {
                        let mut details = Vec::new();

                        let message = message_buf.trim();
                        if !message.is_empty() {
                            details.push(message.to_string());
                        }

                        let stack = stack_buf.trim();
                        if !stack.is_empty() {
                            let stack_lines: Vec<&str> = stack.lines().take(3).collect();
                            if !stack_lines.is_empty() {
                                details.push(stack_lines.join("\n"));
                            }
                        }

                        summary.failed_tests.push(FailedTest {
                            name: failed_test_name.clone(),
                            details,
                        });

                        in_failed_result = false;
                        in_error_info = false;
                        capture_field = None;
                        message_buf.clear();
                        stack_buf.clear();
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }

        buf.clear();
    }

    if !saw_test_run {
        return None;
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
