use crate::binlog;
use crate::dotnet_trx;
use crate::tracking;
use crate::utils::truncate;
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run_build(args: &[String], verbose: u8) -> Result<()> {
    run_dotnet_with_binlog("build", args, verbose)
}

pub fn run_test(args: &[String], verbose: u8) -> Result<()> {
    run_dotnet_with_binlog("test", args, verbose)
}

pub fn run_restore(args: &[String], verbose: u8) -> Result<()> {
    run_dotnet_with_binlog("restore", args, verbose)
}

pub fn run_passthrough(args: &[OsString], verbose: u8) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("dotnet: no subcommand specified");
    }

    let timer = tracking::TimedExecution::start();
    let subcommand = args[0].to_string_lossy().to_string();

    let mut cmd = Command::new("dotnet");
    cmd.arg(&subcommand);
    for arg in &args[1..] {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: dotnet {} ...", subcommand);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run dotnet {}", subcommand))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    print!("{}", stdout);
    eprint!("{}", stderr);

    timer.track(
        &format!("dotnet {}", subcommand),
        &format!("rtk dotnet {}", subcommand),
        &raw,
        &raw,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn run_dotnet_with_binlog(subcommand: &str, args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();
    let binlog_path = build_binlog_path(subcommand);

    // For test commands, also create a TRX file for detailed results
    let trx_path = if subcommand == "test" {
        Some(build_trx_path())
    } else {
        None
    };

    let mut cmd = Command::new("dotnet");
    cmd.arg(subcommand);

    for arg in build_effective_dotnet_args(subcommand, args, &binlog_path, trx_path.as_deref()) {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: dotnet {} {}", subcommand, args.join(" "));
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run dotnet {}", subcommand))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = match subcommand {
        "build" => {
            let binlog_summary = normalize_build_summary(
                binlog::parse_build(&binlog_path)?,
                output.status.success(),
            );
            let raw_summary = normalize_build_summary(
                binlog::parse_build_from_text(&raw),
                output.status.success(),
            );
            let summary = merge_build_summaries(binlog_summary, raw_summary);
            format_build_output(&summary, &binlog_path)
        }
        "test" => {
            // First try to parse from binlog/console output
            let parsed_summary = binlog::parse_test(&binlog_path)?;
            let summary = maybe_fill_test_summary_from_trx(
                parsed_summary,
                trx_path.as_deref(),
                dotnet_trx::find_recent_trx_in_testresults(),
            );

            let summary = normalize_test_summary(summary, output.status.success());
            format_test_output(&summary, &binlog_path)
        }
        "restore" => {
            let summary = binlog::parse_restore(&binlog_path)?;
            format_restore_output(&summary, &binlog_path)
        }
        _ => raw.clone(),
    };

    println!("{}", filtered);

    timer.track(
        &format!("dotnet {} {}", subcommand, args.join(" ")),
        &format!("rtk dotnet {} {}", subcommand, args.join(" ")),
        &raw,
        &filtered,
    );

    if verbose > 0 {
        eprintln!("Binlog saved: {}", binlog_path.display());
    }

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn build_binlog_path(subcommand: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    std::env::temp_dir().join(format!("rtk_dotnet_{}_{}.binlog", subcommand, ts))
}

fn build_trx_path() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    std::env::temp_dir().join(format!("rtk_dotnet_test_{}.trx", ts))
}

fn parse_trx_with_cleanup(path: &Path) -> Option<binlog::TestSummary> {
    let summary = dotnet_trx::parse_trx_file(path)?;
    std::fs::remove_file(path).ok();
    Some(summary)
}

fn maybe_fill_test_summary_from_trx(
    summary: binlog::TestSummary,
    trx_path: Option<&Path>,
    fallback_trx_path: Option<PathBuf>,
) -> binlog::TestSummary {
    if summary.total != 0 || !summary.failed_tests.is_empty() {
        return summary;
    }

    if let Some(trx) = trx_path.filter(|path| path.exists()) {
        if let Some(trx_summary) = parse_trx_with_cleanup(trx) {
            return trx_summary;
        }
    }

    if let Some(trx) = fallback_trx_path {
        if let Some(trx_summary) = dotnet_trx::parse_trx_file(&trx) {
            return trx_summary;
        }
    }

    summary
}

fn build_effective_dotnet_args(
    subcommand: &str,
    args: &[String],
    binlog_path: &Path,
    trx_path: Option<&Path>,
) -> Vec<String> {
    let mut effective = Vec::new();

    if !has_binlog_arg(args) {
        effective.push(format!("-bl:{}", binlog_path.display()));
    }

    if !has_verbosity_arg(args) {
        effective.push("-v:minimal".to_string());
    }

    if !has_nologo_arg(args) {
        effective.push("-nologo".to_string());
    }

    if subcommand == "test" && !has_logger_arg(args) {
        if let Some(trx) = trx_path {
            effective.push("--logger".to_string());
            effective.push(format!("trx;LogFileName=\"{}\"", trx.display()));
        }
    }

    effective.extend(args.iter().cloned());
    effective
}

fn has_binlog_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        let lower = arg.to_ascii_lowercase();
        lower.starts_with("-bl") || lower.starts_with("/bl")
    })
}

fn has_verbosity_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        let lower = arg.to_ascii_lowercase();
        lower.starts_with("-v:") || lower.starts_with("/v:") || lower == "-v" || lower == "/v"
    })
}

fn has_nologo_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        let lower = arg.to_ascii_lowercase();
        lower == "-nologo" || lower == "/nologo"
    })
}

fn has_logger_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        let lower = arg.to_ascii_lowercase();
        lower.starts_with("--logger") || lower.starts_with("-l") || lower.contains("logger")
    })
}

fn normalize_build_summary(
    mut summary: binlog::BuildSummary,
    command_success: bool,
) -> binlog::BuildSummary {
    if command_success {
        summary.succeeded = true;
        if summary.project_count == 0 {
            summary.project_count = 1;
        }
    }

    summary
}

fn merge_build_summaries(
    mut binlog_summary: binlog::BuildSummary,
    raw_summary: binlog::BuildSummary,
) -> binlog::BuildSummary {
    binlog_summary.errors = select_preferred_issues(binlog_summary.errors, raw_summary.errors);
    binlog_summary.warnings =
        select_preferred_issues(binlog_summary.warnings, raw_summary.warnings);

    if binlog_summary.project_count == 0 {
        binlog_summary.project_count = raw_summary.project_count;
    }
    if binlog_summary.duration_text.is_none() {
        binlog_summary.duration_text = raw_summary.duration_text;
    }

    binlog_summary
}

fn select_preferred_issues(
    binlog_issues: Vec<binlog::BinlogIssue>,
    raw_issues: Vec<binlog::BinlogIssue>,
) -> Vec<binlog::BinlogIssue> {
    if binlog_issues.is_empty() {
        return raw_issues;
    }
    if raw_issues.is_empty() {
        return binlog_issues;
    }

    let binlog_score = issues_quality_score(&binlog_issues);
    let raw_score = issues_quality_score(&raw_issues);

    if raw_score > binlog_score
        || (raw_score == binlog_score && raw_issues.len() > binlog_issues.len())
    {
        raw_issues
    } else {
        binlog_issues
    }
}

fn issues_quality_score(issues: &[binlog::BinlogIssue]) -> usize {
    issues.iter().map(issue_quality_score).sum()
}

fn issue_quality_score(issue: &binlog::BinlogIssue) -> usize {
    let mut score = 0;

    if !issue.file.is_empty() && !looks_like_diagnostic_token(&issue.file) {
        score += 4;
    }
    if !issue.code.is_empty() {
        score += 2;
    }
    if issue.line > 0 {
        score += 1;
    }
    if issue.column > 0 {
        score += 1;
    }

    score
}

fn looks_like_diagnostic_token(value: &str) -> bool {
    let mut letters = 0;
    let mut digits = 0;

    for c in value.chars() {
        if c.is_ascii_alphabetic() {
            letters += 1;
        } else if c.is_ascii_digit() {
            digits += 1;
        } else {
            return false;
        }
    }

    letters >= 2 && digits >= 3
}

fn normalize_test_summary(
    mut summary: binlog::TestSummary,
    command_success: bool,
) -> binlog::TestSummary {
    if !command_success && summary.failed == 0 && summary.failed_tests.is_empty() {
        summary.failed = 1;
        if summary.total == 0 {
            summary.total = 1;
        }
    }

    if command_success && summary.total == 0 && summary.passed == 0 {
        summary.project_count = summary.project_count.max(1);
    }

    summary
}

fn format_issue(issue: &binlog::BinlogIssue, kind: &str) -> String {
    if issue.file.is_empty() {
        return format!("  {} {}", kind, truncate(&issue.message, 180));
    }

    format!(
        "  {}({},{}) {} {}: {}",
        issue.file,
        issue.line,
        issue.column,
        kind,
        issue.code,
        truncate(&issue.message, 180)
    )
}

fn format_build_output(summary: &binlog::BuildSummary, binlog_path: &Path) -> String {
    let status_icon = if summary.succeeded { "ok" } else { "fail" };
    let duration = summary.duration_text.as_deref().unwrap_or("unknown");

    let mut out = format!(
        "{} dotnet build: {} projects, {} errors, {} warnings ({})",
        status_icon,
        summary.project_count,
        summary.errors.len(),
        summary.warnings.len(),
        duration
    );

    if !summary.errors.is_empty() {
        out.push_str("\n---------------------------------------\n\nErrors:\n");
        for issue in summary.errors.iter().take(20) {
            out.push_str(&format!("{}\n", format_issue(issue, "error")));
        }
        if summary.errors.len() > 20 {
            out.push_str(&format!(
                "  ... +{} more errors\n",
                summary.errors.len() - 20
            ));
        }
    }

    if !summary.warnings.is_empty() {
        out.push_str("\nWarnings:\n");
        for issue in summary.warnings.iter().take(10) {
            out.push_str(&format!("{}\n", format_issue(issue, "warning")));
        }
        if summary.warnings.len() > 10 {
            out.push_str(&format!(
                "  ... +{} more warnings\n",
                summary.warnings.len() - 10
            ));
        }
    }

    out.push_str(&format!("\nBinlog: {}", binlog_path.display()));
    out
}

fn format_test_output(summary: &binlog::TestSummary, binlog_path: &Path) -> String {
    let has_failures = summary.failed > 0 || !summary.failed_tests.is_empty();
    let status_icon = if has_failures { "fail" } else { "ok" };
    let duration = summary.duration_text.as_deref().unwrap_or("unknown");
    let counts_unavailable = summary.passed == 0
        && summary.failed == 0
        && summary.skipped == 0
        && summary.total == 0
        && summary.failed_tests.is_empty();

    let mut out = if counts_unavailable {
        format!(
            "{} dotnet test: completed (binlog-only mode, counts unavailable) ({})",
            status_icon, duration
        )
    } else if has_failures {
        format!(
            "{} dotnet test: {} passed, {} failed, {} skipped in {} projects ({})",
            status_icon,
            summary.passed,
            summary.failed,
            summary.skipped,
            summary.project_count,
            duration
        )
    } else {
        format!(
            "{} dotnet test: {} tests passed in {} projects ({})",
            status_icon, summary.passed, summary.project_count, duration
        )
    };

    if has_failures && !summary.failed_tests.is_empty() {
        out.push_str("\n---------------------------------------\n\nFailed Tests:\n");
        for failed in summary.failed_tests.iter().take(15) {
            out.push_str(&format!("  {}\n", failed.name));
            for detail in &failed.details {
                out.push_str(&format!("    {}\n", truncate(detail, 180)));
            }
            out.push('\n');
        }
        if summary.failed_tests.len() > 15 {
            out.push_str(&format!(
                "... +{} more failed tests\n",
                summary.failed_tests.len() - 15
            ));
        }
    }

    out.push_str(&format!("\nBinlog: {}", binlog_path.display()));
    out
}

fn format_restore_output(summary: &binlog::RestoreSummary, binlog_path: &Path) -> String {
    let has_errors = summary.errors > 0;
    let status_icon = if has_errors { "fail" } else { "ok" };
    let duration = summary.duration_text.as_deref().unwrap_or("unknown");

    let mut out = format!(
        "{} dotnet restore: {} projects, {} errors, {} warnings ({})",
        status_icon, summary.restored_projects, summary.errors, summary.warnings, duration
    );
    out.push_str(&format!("\nBinlog: {}", binlog_path.display()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn build_dotnet_args_for_test(
        subcommand: &str,
        args: &[String],
        with_trx: bool,
    ) -> Vec<String> {
        let binlog_path = Path::new("/tmp/test.binlog");
        let trx_path = if with_trx {
            Some(Path::new("/tmp/test results/test.trx"))
        } else {
            None
        };

        build_effective_dotnet_args(subcommand, args, binlog_path, trx_path)
    }

    fn trx_with_counts(total: usize, passed: usize, failed: usize) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
  <ResultSummary outcome="Completed">
    <Counters total="{}" executed="{}" passed="{}" failed="{}" error="0" />
  </ResultSummary>
</TestRun>"#,
            total, total, passed, failed
        )
    }

    #[test]
    fn test_has_binlog_arg_detects_variants() {
        let args = vec!["-bl:my.binlog".to_string()];
        assert!(has_binlog_arg(&args));

        let args = vec!["/bl".to_string()];
        assert!(has_binlog_arg(&args));

        let args = vec!["--configuration".to_string(), "Release".to_string()];
        assert!(!has_binlog_arg(&args));
    }

    #[test]
    fn test_format_build_output_includes_errors_and_warnings() {
        let summary = binlog::BuildSummary {
            succeeded: false,
            project_count: 2,
            errors: vec![binlog::BinlogIssue {
                code: "CS0103".to_string(),
                file: "src/Program.cs".to_string(),
                line: 42,
                column: 15,
                message: "The name 'foo' does not exist".to_string(),
            }],
            warnings: vec![binlog::BinlogIssue {
                code: "CS0219".to_string(),
                file: "src/Program.cs".to_string(),
                line: 25,
                column: 10,
                message: "Variable 'x' is assigned but never used".to_string(),
            }],
            duration_text: Some("00:00:04.20".to_string()),
        };

        let output = format_build_output(&summary, Path::new("/tmp/build.binlog"));
        assert!(output.contains("dotnet build: 2 projects, 1 errors, 1 warnings"));
        assert!(output.contains("error CS0103"));
        assert!(output.contains("warning CS0219"));
    }

    #[test]
    fn test_format_test_output_shows_failures() {
        let summary = binlog::TestSummary {
            passed: 10,
            failed: 1,
            skipped: 0,
            total: 11,
            project_count: 1,
            failed_tests: vec![binlog::FailedTest {
                name: "MyTests.ShouldFail".to_string(),
                details: vec!["Assert.Equal failure".to_string()],
            }],
            duration_text: Some("1 s".to_string()),
        };

        let output = format_test_output(&summary, Path::new("/tmp/test.binlog"));
        assert!(output.contains("10 passed, 1 failed"));
        assert!(output.contains("MyTests.ShouldFail"));
    }

    #[test]
    fn test_format_restore_output_success() {
        let summary = binlog::RestoreSummary {
            restored_projects: 3,
            warnings: 1,
            errors: 0,
            duration_text: Some("00:00:01.10".to_string()),
        };

        let output = format_restore_output(&summary, Path::new("/tmp/restore.binlog"));
        assert!(output.starts_with("ok dotnet restore"));
        assert!(output.contains("3 projects"));
        assert!(output.contains("1 warnings"));
    }

    #[test]
    fn test_format_test_output_handles_binlog_only_without_counts() {
        let summary = binlog::TestSummary {
            passed: 0,
            failed: 0,
            skipped: 0,
            total: 0,
            project_count: 0,
            failed_tests: Vec::new(),
            duration_text: Some("unknown".to_string()),
        };

        let output = format_test_output(&summary, Path::new("/tmp/test.binlog"));
        assert!(output.contains("counts unavailable"));
    }

    #[test]
    fn test_normalize_build_summary_sets_success_floor() {
        let summary = binlog::BuildSummary {
            succeeded: false,
            project_count: 0,
            errors: Vec::new(),
            warnings: Vec::new(),
            duration_text: None,
        };

        let normalized = normalize_build_summary(summary, true);
        assert!(normalized.succeeded);
        assert_eq!(normalized.project_count, 1);
    }

    #[test]
    fn test_merge_build_summaries_prefers_raw_when_binlog_loses_context() {
        let binlog_summary = binlog::BuildSummary {
            succeeded: false,
            project_count: 11,
            errors: vec![binlog::BinlogIssue {
                code: String::new(),
                file: "IDE0055".to_string(),
                line: 0,
                column: 0,
                message: "Fix formatting".to_string(),
            }],
            warnings: Vec::new(),
            duration_text: Some("00:00:03.54".to_string()),
        };

        let raw_summary = binlog::BuildSummary {
            succeeded: false,
            project_count: 2,
            errors: vec![
                binlog::BinlogIssue {
                    code: "IDE0055".to_string(),
                    file: "/repo/src/Behavior.cs".to_string(),
                    line: 13,
                    column: 32,
                    message: "Fix formatting".to_string(),
                },
                binlog::BinlogIssue {
                    code: "IDE0055".to_string(),
                    file: "/repo/src/Behavior.cs".to_string(),
                    line: 13,
                    column: 41,
                    message: "Fix formatting".to_string(),
                },
            ],
            warnings: Vec::new(),
            duration_text: Some("00:00:03.54".to_string()),
        };

        let merged = merge_build_summaries(binlog_summary, raw_summary);
        assert_eq!(merged.project_count, 11);
        assert_eq!(merged.errors.len(), 2);
        assert_eq!(merged.errors[0].line, 13);
        assert_eq!(merged.errors[0].column, 32);
    }

    #[test]
    fn test_merge_build_summaries_keeps_binlog_when_context_is_good() {
        let binlog_summary = binlog::BuildSummary {
            succeeded: false,
            project_count: 2,
            errors: vec![binlog::BinlogIssue {
                code: "CS0103".to_string(),
                file: "src/Program.cs".to_string(),
                line: 42,
                column: 15,
                message: "The name 'foo' does not exist".to_string(),
            }],
            warnings: Vec::new(),
            duration_text: Some("00:00:01.00".to_string()),
        };

        let raw_summary = binlog::BuildSummary {
            succeeded: false,
            project_count: 2,
            errors: vec![binlog::BinlogIssue {
                code: "CS0103".to_string(),
                file: String::new(),
                line: 0,
                column: 0,
                message: "Build error #1 (details omitted)".to_string(),
            }],
            warnings: Vec::new(),
            duration_text: None,
        };

        let merged = merge_build_summaries(binlog_summary.clone(), raw_summary);
        assert_eq!(merged.errors, binlog_summary.errors);
    }

    #[test]
    fn test_normalize_test_summary_sets_failure_floor() {
        let summary = binlog::TestSummary {
            passed: 0,
            failed: 0,
            skipped: 0,
            total: 0,
            project_count: 0,
            failed_tests: Vec::new(),
            duration_text: None,
        };

        let normalized = normalize_test_summary(summary, false);
        assert_eq!(normalized.failed, 1);
        assert_eq!(normalized.total, 1);
    }

    #[test]
    fn test_parse_trx_with_cleanup_deletes_file_after_parse() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let trx_path = temp_dir.path().join("results.trx");
        let trx = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
  <ResultSummary outcome="Completed">
    <Counters total="2" executed="2" passed="2" failed="0" error="0" />
  </ResultSummary>
</TestRun>"#;
        fs::write(&trx_path, trx).expect("write trx");

        let summary = parse_trx_with_cleanup(&trx_path);
        assert!(summary.is_some());
        assert!(!trx_path.exists());
    }

    #[test]
    fn test_parse_trx_with_cleanup_non_existent_path_returns_none() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let trx_path = temp_dir.path().join("missing.trx");

        let summary = parse_trx_with_cleanup(&trx_path);
        assert!(summary.is_none());
    }

    #[test]
    fn test_forwarding_args_with_spaces() {
        let args = vec![
            "--filter".to_string(),
            "FullyQualifiedName~MyTests.Calculator*".to_string(),
            "-c".to_string(),
            "Release".to_string(),
        ];

        let injected = build_dotnet_args_for_test("test", &args, true);
        assert!(injected.contains(&"--filter".to_string()));
        assert!(injected.contains(&"FullyQualifiedName~MyTests.Calculator*".to_string()));
        assert!(injected.contains(&"-c".to_string()));
        assert!(injected.contains(&"Release".to_string()));
    }

    #[test]
    fn test_forwarding_config_and_framework() {
        let args = vec![
            "--configuration".to_string(),
            "Release".to_string(),
            "--framework".to_string(),
            "net8.0".to_string(),
        ];

        let injected = build_dotnet_args_for_test("test", &args, true);
        assert!(injected.contains(&"--configuration".to_string()));
        assert!(injected.contains(&"Release".to_string()));
        assert!(injected.contains(&"--framework".to_string()));
        assert!(injected.contains(&"net8.0".to_string()));
    }

    #[test]
    fn test_forwarding_project_file() {
        let args = vec![
            "--project".to_string(),
            "src/My App.Tests/My App.Tests.csproj".to_string(),
        ];

        let injected = build_dotnet_args_for_test("test", &args, true);
        assert!(injected.contains(&"--project".to_string()));
        assert!(injected.contains(&"src/My App.Tests/My App.Tests.csproj".to_string()));
    }

    #[test]
    fn test_forwarding_no_build_and_no_restore() {
        let args = vec!["--no-build".to_string(), "--no-restore".to_string()];

        let injected = build_dotnet_args_for_test("test", &args, true);
        assert!(injected.contains(&"--no-build".to_string()));
        assert!(injected.contains(&"--no-restore".to_string()));
    }

    #[test]
    fn test_user_verbose_override() {
        let args = vec!["-v:detailed".to_string()];

        let injected = build_dotnet_args_for_test("test", &args, true);
        let verbose_count = injected.iter().filter(|a| a.starts_with("-v:")).count();
        assert_eq!(verbose_count, 1);
        assert!(injected.contains(&"-v:detailed".to_string()));
        assert!(!injected.contains(&"-v:minimal".to_string()));
    }

    #[test]
    fn test_user_logger_override() {
        let args = vec![
            "--logger".to_string(),
            "console;verbosity=detailed".to_string(),
        ];

        let injected = build_dotnet_args_for_test("test", &args, true);
        assert!(injected.contains(&"--logger".to_string()));
        assert!(injected.contains(&"console;verbosity=detailed".to_string()));
        assert!(!injected.iter().any(|a| a.contains("trx;LogFileName=")));
    }

    #[test]
    fn test_trx_logger_path_is_quoted_when_path_contains_spaces() {
        let args = Vec::<String>::new();

        let injected = build_dotnet_args_for_test("test", &args, true);
        let trx_arg = injected
            .iter()
            .find(|a| a.starts_with("trx;LogFileName="))
            .expect("trx logger argument exists");

        assert!(trx_arg.contains("LogFileName=\"/tmp/test results/test.trx\""));
    }

    #[test]
    fn test_maybe_fill_test_summary_from_trx_uses_primary_and_cleans_file() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let primary = temp_dir.path().join("primary.trx");
        fs::write(&primary, trx_with_counts(3, 3, 0)).expect("write primary trx");

        let filled =
            maybe_fill_test_summary_from_trx(binlog::TestSummary::default(), Some(&primary), None);

        assert_eq!(filled.total, 3);
        assert_eq!(filled.passed, 3);
        assert!(!primary.exists());
    }

    #[test]
    fn test_maybe_fill_test_summary_from_trx_falls_back_to_testresults() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let fallback = temp_dir.path().join("fallback.trx");
        fs::write(&fallback, trx_with_counts(2, 1, 1)).expect("write fallback trx");
        let missing_primary = temp_dir.path().join("missing.trx");

        let filled = maybe_fill_test_summary_from_trx(
            binlog::TestSummary::default(),
            Some(&missing_primary),
            Some(fallback.clone()),
        );

        assert_eq!(filled.total, 2);
        assert_eq!(filled.failed, 1);
        assert!(fallback.exists());
    }
}
