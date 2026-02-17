use anyhow::{Context, Result};
use flate2::read::{DeflateDecoder, GzDecoder};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinlogIssue {
    pub code: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct BuildSummary {
    pub succeeded: bool,
    pub project_count: usize,
    pub errors: Vec<BinlogIssue>,
    pub warnings: Vec<BinlogIssue>,
    pub duration_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedTest {
    pub name: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TestSummary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total: usize,
    pub project_count: usize,
    pub failed_tests: Vec<FailedTest>,
    pub duration_text: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RestoreSummary {
    pub restored_projects: usize,
    pub warnings: usize,
    pub errors: usize,
    pub duration_text: Option<String>,
}

lazy_static! {
    static ref ISSUE_RE: Regex = Regex::new(
        r"(?m)^\s*(?P<file>[^\r\n:(]+)\((?P<line>\d+),(?P<column>\d+)\):\s*(?P<kind>error|warning)\s*(?P<code>[A-Za-z]+\d+):\s*(?P<msg>.+)$"
    )
    .expect("valid regex");
    static ref BUILD_SUMMARY_RE: Regex = Regex::new(r"(?m)^\s*(?P<count>\d+)\s+(?P<kind>Warning|Error)\(s\)")
        .expect("valid regex");
    static ref DURATION_RE: Regex =
        Regex::new(r"(?m)^\s*Time Elapsed\s+(?P<duration>[^\r\n]+)$").expect("valid regex");
    static ref TEST_RESULT_RE: Regex = Regex::new(
        r"(?m)(?:Passed!|Failed!)\s*-\s*Failed:\s*(?P<failed>\d+),\s*Passed:\s*(?P<passed>\d+),\s*Skipped:\s*(?P<skipped>\d+),\s*Total:\s*(?P<total>\d+),\s*Duration:\s*(?P<duration>[^\r\n-]+)"
    )
    .expect("valid regex");
    static ref FAILED_TEST_HEAD_RE: Regex =
        Regex::new(r"(?m)^\s*Failed\s+(?P<name>[^\r\n\[]+)").expect("valid regex");
    static ref RESTORE_PROJECT_RE: Regex =
        Regex::new(r"(?m)^\s*Restored\s+.+\.csproj\s*\(").expect("valid regex");
    static ref WARNING_COUNT_RE: Regex = Regex::new(r"(?m)^\s*warning\s+").expect("valid regex");
    static ref ERROR_COUNT_RE: Regex = Regex::new(r"(?m)^\s*error\s+").expect("valid regex");
    static ref PROJECT_PATH_RE: Regex =
        Regex::new(r"(?m)^\s*([A-Za-z]:)?[^\r\n]*\.csproj(?:\s|$)").expect("valid regex");
    static ref PRINTABLE_RUN_RE: Regex = Regex::new(r"[\x20-\x7E]{5,}").expect("valid regex");
    static ref DIAGNOSTIC_CODE_RE: Regex =
        Regex::new(r"^[A-Za-z]{2,}\d{3,}$").expect("valid regex");
    static ref SOURCE_FILE_RE: Regex = Regex::new(r"(?i)([A-Za-z]:)?[/\\][^\s]+\.(cs|vb|fs)")
        .expect("valid regex");
    // TRX (Visual Studio Test Results) parsing
    // Note: (?s) enables DOTALL mode so . matches newlines
    static ref TRX_COUNTERS_RE: Regex = Regex::new(
        r#"<Counters\s+total="(?P<total>\d+)"\s+executed="(?P<executed>\d+)"\s+passed="(?P<passed>\d+)"\s+failed="(?P<failed>\d+)""#
    ).expect("valid regex");
    static ref TRX_TEST_RESULT_RE: Regex = Regex::new(
        r#"(?s)<UnitTestResult[^>]*testName="(?P<name>[^"]+)"[^>]*outcome="(?P<outcome>[^"]+)"[^>]*>(.*?)</UnitTestResult>"#
    ).expect("valid regex");
    static ref TRX_ERROR_MESSAGE_RE: Regex = Regex::new(
        r#"(?s)<ErrorInfo>.*?<Message>(?P<message>.*?)</Message>.*?<StackTrace>(?P<stack>.*?)</StackTrace>.*?</ErrorInfo>"#
    ).expect("valid regex");
}

const SENSITIVE_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "USERPROFILE",
    "USERNAME",
    "USER",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_LAUNCHER",
    "GITHUB_TOKEN",
    "NUGET_API_KEY",
    "AZURE_DEVOPS_TOKEN",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "DOCKER_CONFIG",
    "KUBECONFIG",
];

pub fn parse_build(binlog_path: &Path, fallback_output: &str) -> Result<BuildSummary> {
    let source = load_binlog_text(binlog_path).unwrap_or_else(|| fallback_output.to_string());
    Ok(parse_build_from_text(&source))
}

pub fn parse_test(binlog_path: &Path, fallback_output: &str) -> Result<TestSummary> {
    let source = load_binlog_text(binlog_path).unwrap_or_else(|| fallback_output.to_string());
    Ok(parse_test_from_text(&source))
}

pub fn parse_restore(binlog_path: &Path, fallback_output: &str) -> Result<RestoreSummary> {
    let source = load_binlog_text(binlog_path).unwrap_or_else(|| fallback_output.to_string());
    Ok(parse_restore_from_text(&source))
}

pub fn scrub_sensitive_env_vars(input: &str) -> String {
    let mut output = input.to_string();

    for key in SENSITIVE_ENV_VARS {
        let escaped_key = regex::escape(key);

        let equals_pattern = format!(r"(?P<prefix>\b{}\s*=\s*)(?P<value>[^\s;]+)", escaped_key);
        if let Ok(re) = Regex::new(&equals_pattern) {
            output = re.replace_all(&output, "${prefix}[REDACTED]").into_owned();
        }

        let colon_pattern = format!(r"(?P<prefix>\b{}\s*:\s*)(?P<value>[^\s;]+)", escaped_key);
        if let Ok(re) = Regex::new(&colon_pattern) {
            output = re.replace_all(&output, "${prefix}[REDACTED]").into_owned();
        }
    }

    output
}

pub fn parse_build_from_text(text: &str) -> BuildSummary {
    let scrubbed = scrub_sensitive_env_vars(text);
    let mut seen_errors: HashSet<(String, String, u32, u32, String)> = HashSet::new();
    let mut seen_warnings: HashSet<(String, String, u32, u32, String)> = HashSet::new();
    let mut summary = BuildSummary {
        succeeded: scrubbed.contains("Build succeeded") && !scrubbed.contains("Build FAILED"),
        project_count: count_projects(&scrubbed),
        errors: Vec::new(),
        warnings: Vec::new(),
        duration_text: extract_duration(&scrubbed),
    };

    for captures in ISSUE_RE.captures_iter(&scrubbed) {
        let issue = BinlogIssue {
            code: captures
                .name("code")
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            file: captures
                .name("file")
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            line: captures
                .name("line")
                .and_then(|m| m.as_str().parse::<u32>().ok())
                .unwrap_or(0),
            column: captures
                .name("column")
                .and_then(|m| m.as_str().parse::<u32>().ok())
                .unwrap_or(0),
            message: captures
                .name("msg")
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default(),
        };

        let key = (
            issue.code.clone(),
            issue.file.clone(),
            issue.line,
            issue.column,
            issue.message.clone(),
        );

        match captures.name("kind").map(|m| m.as_str()) {
            Some("error") => {
                if seen_errors.insert(key) {
                    summary.errors.push(issue);
                }
            }
            Some("warning") => {
                if seen_warnings.insert(key) {
                    summary.warnings.push(issue);
                }
            }
            _ => {}
        }
    }

    if summary.errors.is_empty() || summary.warnings.is_empty() {
        let mut warning_count_from_summary = None;
        let mut error_count_from_summary = None;

        for captures in BUILD_SUMMARY_RE.captures_iter(&scrubbed) {
            let count = captures
                .name("count")
                .and_then(|m| m.as_str().parse::<usize>().ok())
                .unwrap_or(0);

            match captures.name("kind").map(|m| m.as_str()) {
                Some("Warning") => warning_count_from_summary = Some(count),
                Some("Error") => error_count_from_summary = Some(count),
                _ => {}
            }
        }

        if summary.errors.is_empty() {
            for idx in 0..error_count_from_summary.unwrap_or(0) {
                summary.errors.push(BinlogIssue {
                    code: String::new(),
                    file: String::new(),
                    line: 0,
                    column: 0,
                    message: format!("Build error #{} (details omitted)", idx + 1),
                });
            }
        }

        if summary.warnings.is_empty() {
            for idx in 0..warning_count_from_summary.unwrap_or(0) {
                summary.warnings.push(BinlogIssue {
                    code: String::new(),
                    file: String::new(),
                    line: 0,
                    column: 0,
                    message: format!("Build warning #{} (details omitted)", idx + 1),
                });
            }
        }
    }

    if summary.errors.is_empty() {
        summary.errors = extract_binary_like_issues(&scrubbed);
    }

    if summary.project_count == 0
        && (scrubbed.contains("Build succeeded")
            || scrubbed.contains("Build FAILED")
            || scrubbed.contains(" -> "))
    {
        summary.project_count = 1;
    }

    summary
}

pub fn parse_test_from_text(text: &str) -> TestSummary {
    let scrubbed = scrub_sensitive_env_vars(text);
    let mut summary = TestSummary {
        passed: 0,
        failed: 0,
        skipped: 0,
        total: 0,
        project_count: count_projects(&scrubbed).max(1),
        failed_tests: Vec::new(),
        duration_text: extract_duration(&scrubbed),
    };

    if let Some(captures) = TEST_RESULT_RE.captures(&scrubbed) {
        summary.passed = captures
            .name("passed")
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        summary.failed = captures
            .name("failed")
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        summary.skipped = captures
            .name("skipped")
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        summary.total = captures
            .name("total")
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        if let Some(duration) = captures.name("duration") {
            summary.duration_text = Some(duration.as_str().trim().to_string());
        }
    }

    let lines: Vec<&str> = scrubbed.lines().collect();
    let mut idx = 0;
    while idx < lines.len() {
        let line = lines[idx];
        if let Some(captures) = FAILED_TEST_HEAD_RE.captures(line) {
            let name = captures
                .name("name")
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let mut details = Vec::new();
            idx += 1;
            while idx < lines.len() {
                let detail_line = lines[idx].trim_end();
                if detail_line.trim().is_empty() {
                    break;
                }
                if FAILED_TEST_HEAD_RE.is_match(detail_line) {
                    idx = idx.saturating_sub(1);
                    break;
                }
                if detail_line.trim_start().starts_with("Failed ")
                    || detail_line.trim_start().starts_with("Passed ")
                {
                    idx = idx.saturating_sub(1);
                    break;
                }

                details.push(detail_line.trim().to_string());
                if details.len() >= 4 {
                    break;
                }
                idx += 1;
            }
            summary.failed_tests.push(FailedTest { name, details });
        }
        idx += 1;
    }

    if summary.failed == 0 {
        summary.failed = summary.failed_tests.len();
    }
    if summary.total == 0 {
        summary.total = summary.passed + summary.failed + summary.skipped;
    }

    summary
}

pub fn parse_restore_from_text(text: &str) -> RestoreSummary {
    let scrubbed = scrub_sensitive_env_vars(text);
    RestoreSummary {
        restored_projects: RESTORE_PROJECT_RE.captures_iter(&scrubbed).count(),
        warnings: WARNING_COUNT_RE.captures_iter(&scrubbed).count(),
        errors: ERROR_COUNT_RE.captures_iter(&scrubbed).count(),
        duration_text: extract_duration(&scrubbed),
    }
}

fn count_projects(text: &str) -> usize {
    PROJECT_PATH_RE.captures_iter(text).count()
}

fn extract_duration(text: &str) -> Option<String> {
    DURATION_RE
        .captures(text)
        .and_then(|c| c.name("duration"))
        .map(|m| m.as_str().trim().to_string())
}

fn load_binlog_text(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }

    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read binlog at {}", path.display()))
        .ok()?;

    if bytes.is_empty() {
        return None;
    }

    if let Some(decoded) = try_gzip_decode(&bytes) {
        let text = String::from_utf8_lossy(&decoded).into_owned();
        if looks_like_console_output(&text) {
            return Some(text);
        }
    }

    if let Some(decoded) = try_deflate_decode(&bytes) {
        let text = String::from_utf8_lossy(&decoded).into_owned();
        if looks_like_console_output(&text) {
            return Some(text);
        }
    }

    let plain = String::from_utf8_lossy(&bytes).into_owned();
    if looks_like_console_output(&plain) {
        return Some(plain);
    }

    None
}

fn looks_like_console_output(text: &str) -> bool {
    let markers = [
        "Build succeeded",
        "Build FAILED",
        "Passed!",
        "Failed!",
        "Time Elapsed",
        ".csproj",
        ": error ",
        ": warning ",
        "Restored ",
    ];

    markers.iter().any(|marker| text.contains(marker))
}

fn try_gzip_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = GzDecoder::new(bytes);
    let mut output = Vec::new();
    if decoder.read_to_end(&mut output).is_ok() && !output.is_empty() {
        return Some(output);
    }
    None
}

fn try_deflate_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(bytes);
    let mut output = Vec::new();
    if decoder.read_to_end(&mut output).is_ok() && !output.is_empty() {
        return Some(output);
    }
    None
}

fn extract_printable_runs(text: &str) -> Vec<String> {
    let mut runs = Vec::new();
    for captures in PRINTABLE_RUN_RE.captures_iter(text) {
        let Some(matched) = captures.get(0) else {
            continue;
        };

        let run = matched.as_str().trim();
        if run.len() < 5 {
            continue;
        }
        runs.push(run.to_string());
    }
    runs
}

fn extract_binary_like_issues(text: &str) -> Vec<BinlogIssue> {
    let runs = extract_printable_runs(text);
    if runs.is_empty() {
        return Vec::new();
    }

    let mut issues = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new();

    for idx in 0..runs.len() {
        let code = runs[idx].trim();
        if !DIAGNOSTIC_CODE_RE.is_match(code) || !is_likely_diagnostic_code(code) {
            continue;
        }

        let message = (1..=4)
            .filter_map(|delta| idx.checked_sub(delta))
            .map(|j| runs[j].trim())
            .find(|candidate| {
                !DIAGNOSTIC_CODE_RE.is_match(candidate)
                    && !SOURCE_FILE_RE.is_match(candidate)
                    && candidate.chars().any(|c| c.is_ascii_alphabetic())
                    && candidate.contains(' ')
                    && !candidate.contains("Copyright")
                    && !candidate.contains("Compiler version")
            })
            .unwrap_or("Build issue")
            .to_string();

        let file = (1..=4)
            .filter_map(|delta| runs.get(idx + delta))
            .find_map(|candidate| {
                SOURCE_FILE_RE
                    .captures(candidate)
                    .and_then(|caps| caps.get(0))
                    .map(|m| m.as_str().to_string())
            })
            .unwrap_or_default();

        if file.is_empty() && message == "Build issue" {
            continue;
        }

        let key = (code.to_string(), file.clone(), message.clone());
        if !seen.insert(key) {
            continue;
        }

        issues.push(BinlogIssue {
            code: code.to_string(),
            file,
            line: 0,
            column: 0,
            message,
        });
    }

    issues
}

fn is_likely_diagnostic_code(code: &str) -> bool {
    const ALLOWED_PREFIXES: &[&str] = &[
        "CS", "MSB", "NU", "FS", "BC", "CA", "SA", "IDE", "IL", "VB", "AD", "TS", "C", "LNK",
    ];

    ALLOWED_PREFIXES
        .iter()
        .any(|prefix| code.starts_with(prefix))
}

/// Parse TRX (Visual Studio Test Results) file to extract test summary.
/// Returns None if the file doesn't exist or isn't a valid TRX file.
pub fn parse_trx_file(path: &Path) -> Option<TestSummary> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_trx_content(&content)
}

fn parse_trx_content(content: &str) -> Option<TestSummary> {
    // Quick check if this looks like a TRX file
    if !content.contains("<TestRun") || !content.contains("</TestRun>") {
        return None;
    }

    let mut summary = TestSummary::default();

    // Extract counters from ResultSummary
    if let Some(captures) = TRX_COUNTERS_RE.captures(content) {
        summary.total = captures
            .name("total")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        summary.passed = captures
            .name("passed")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        summary.failed = captures
            .name("failed")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
    }

    // Extract failed tests with details
    for captures in TRX_TEST_RESULT_RE.captures_iter(content) {
        let outcome = captures
            .name("outcome")
            .map(|m| m.as_str())
            .unwrap_or("Unknown");

        if outcome != "Failed" {
            continue;
        }

        let name = captures
            .name("name")
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let full_match = captures.get(0).map(|m| m.as_str()).unwrap_or("");
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

    #[test]
    fn test_scrub_sensitive_env_vars_masks_values() {
        let input = "PATH=/usr/local/bin HOME: /Users/daniel GITHUB_TOKEN=ghp_123";
        let scrubbed = scrub_sensitive_env_vars(input);

        assert!(scrubbed.contains("PATH=[REDACTED]"));
        assert!(scrubbed.contains("HOME: [REDACTED]"));
        assert!(scrubbed.contains("GITHUB_TOKEN=[REDACTED]"));
        assert!(!scrubbed.contains("/usr/local/bin"));
        assert!(!scrubbed.contains("ghp_123"));
    }

    #[test]
    fn test_parse_build_from_text_extracts_issues() {
        let input = r#"
Build FAILED.
src/Program.cs(42,15): error CS0103: The name 'foo' does not exist
src/Program.cs(25,10): warning CS0219: Variable 'x' is assigned but never used
    1 Warning(s)
    1 Error(s)
Time Elapsed 00:00:03.45
"#;

        let summary = parse_build_from_text(input);
        assert!(!summary.succeeded);
        assert_eq!(summary.errors.len(), 1);
        assert_eq!(summary.warnings.len(), 1);
        assert_eq!(summary.errors[0].code, "CS0103");
        assert_eq!(summary.warnings[0].code, "CS0219");
        assert_eq!(summary.duration_text.as_deref(), Some("00:00:03.45"));
    }

    #[test]
    fn test_parse_test_from_text_extracts_failure_summary() {
        let input = r#"
Failed!  - Failed:     2, Passed:   245, Skipped:     0, Total:   247, Duration: 1 s
  Failed MyApp.Tests.UnitTests.CalculatorTests.Add_ShouldReturnSum [5 ms]
  Error Message:
   Assert.Equal() Failure: Expected 5, Actual 4

  Failed MyApp.Tests.IntegrationTests.DatabaseTests.CanConnect [20 ms]
  Error Message:
   System.InvalidOperationException: Connection refused
"#;

        let summary = parse_test_from_text(input);
        assert_eq!(summary.passed, 245);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.total, 247);
        assert_eq!(summary.failed_tests.len(), 2);
        assert!(summary.failed_tests[0]
            .name
            .contains("CalculatorTests.Add_ShouldReturnSum"));
    }

    #[test]
    fn test_parse_restore_from_text_extracts_project_count() {
        let input = r#"
  Restored /tmp/App/App.csproj (in 1.1 sec).
  Restored /tmp/App.Tests/App.Tests.csproj (in 1.2 sec).
"#;

        let summary = parse_restore_from_text(input);
        assert_eq!(summary.restored_projects, 2);
        assert_eq!(summary.errors, 0);
    }

    #[test]
    fn test_parse_build_uses_fallback_when_binlog_is_binary() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let binlog_path = temp_dir.path().join("build.binlog");
        std::fs::write(&binlog_path, [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00])
            .expect("write binary file");

        let fallback = include_str!("../tests/fixtures/dotnet/build_failed.txt");
        let summary = parse_build(&binlog_path, fallback).expect("parse should not fail");

        assert_eq!(summary.errors.len(), 1);
        assert_eq!(summary.warnings.len(), 0);
        assert_eq!(summary.errors[0].code, "CS1525");
    }

    #[test]
    fn test_parse_build_from_fixture_text() {
        let input = include_str!("../tests/fixtures/dotnet/build_failed.txt");
        let summary = parse_build_from_text(input);

        assert_eq!(summary.errors.len(), 1);
        assert_eq!(summary.errors[0].code, "CS1525");
        assert_eq!(summary.duration_text.as_deref(), Some("00:00:00.76"));
    }

    #[test]
    fn test_parse_build_sets_project_count_floor() {
        let input = r#"
RtkDotnetSmoke -> /tmp/RtkDotnetSmoke.dll

Build succeeded.
    0 Warning(s)
    0 Error(s)

Time Elapsed 00:00:00.12
"#;

        let summary = parse_build_from_text(input);
        assert_eq!(summary.project_count, 1);
        assert!(summary.succeeded);
    }

    #[test]
    fn test_parse_test_from_fixture_text() {
        let input = include_str!("../tests/fixtures/dotnet/test_failed.txt");
        let summary = parse_test_from_text(input);

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.passed, 0);
        assert_eq!(summary.total, 1);
        assert_eq!(summary.failed_tests.len(), 1);
        assert!(summary.failed_tests[0]
            .name
            .contains("RtkDotnetSmoke.UnitTest1.Test1"));
    }

    #[test]
    fn test_extract_binary_like_issues_recovers_code_message_and_path() {
        let noisy =
            "\x0bInvalid expression term ';'\x18\x06CS1525\x18%/tmp/RtkDotnetSmoke/Broken.cs\x09";
        let issues = extract_binary_like_issues(noisy);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "CS1525");
        assert_eq!(issues[0].file, "/tmp/RtkDotnetSmoke/Broken.cs");
        assert!(issues[0].message.contains("Invalid expression term"));
    }

    #[test]
    fn test_is_likely_diagnostic_code_filters_framework_monikers() {
        assert!(is_likely_diagnostic_code("CS1525"));
        assert!(is_likely_diagnostic_code("MSB4018"));
        assert!(!is_likely_diagnostic_code("NET451"));
        assert!(!is_likely_diagnostic_code("NET10"));
    }

    #[test]
    fn test_parse_trx_content_extracts_passed_counts() {
        let trx = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
  <ResultSummary outcome="Completed">
    <Counters total="5" executed="5" passed="5" failed="0" error="0" />
  </ResultSummary>
</TestRun>"#;

        let summary = parse_trx_content(trx).expect("valid TRX");
        assert_eq!(summary.total, 5);
        assert_eq!(summary.passed, 5);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.failed_tests.len(), 0);
    }

    #[test]
    fn test_parse_trx_content_extracts_failed_tests_with_details() {
        let trx = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
  <Results>
    <UnitTestResult testName="MyTest.ShouldFail" outcome="Failed">
      <Output>
        <ErrorInfo>
          <Message>Expected 2 but was 3</Message>
          <StackTrace>at MyTest.ShouldFail() in /src/Test.cs:line 10</StackTrace>
        </ErrorInfo>
      </Output>
    </UnitTestResult>
  </Results>
  <ResultSummary outcome="Failed">
    <Counters total="3" executed="3" passed="2" failed="1" error="0" />
  </ResultSummary>
</TestRun>"#;

        let summary = parse_trx_content(trx).expect("valid TRX");
        assert_eq!(summary.total, 3);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failed_tests.len(), 1);
        assert_eq!(summary.failed_tests[0].name, "MyTest.ShouldFail");
        assert!(summary.failed_tests[0].details[0].contains("Expected 2"));
    }

    #[test]
    fn test_parse_trx_content_returns_none_for_invalid_xml() {
        let not_trx = "This is not a TRX file";
        assert!(parse_trx_content(not_trx).is_none());
    }
}
