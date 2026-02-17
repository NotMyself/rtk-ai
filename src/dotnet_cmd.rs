use crate::binlog;
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

    if !has_binlog_arg(args) {
        cmd.arg(format!("-bl:{}", binlog_path.display()));
    }

    if !has_verbosity_arg(args) {
        cmd.arg("-v:minimal");
    }

    if !has_nologo_arg(args) {
        cmd.arg("-nologo");
    }

    // Add TRX logger for test commands if not already specified
    if let Some(ref trx) = trx_path {
        if !has_logger_arg(args) {
            cmd.arg(format!("--logger"));
            cmd.arg(format!("trx;LogFileName={}", trx.display()));
        }
    }

    for arg in args {
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
            let summary = normalize_build_summary(
                binlog::parse_build(&binlog_path, &raw)?,
                output.status.success(),
            );
            format_build_output(&summary, &binlog_path)
        }
        "test" => {
            // First try to parse from binlog/console output
            let mut summary = binlog::parse_test(&binlog_path, &raw)?;

            // If binlog parsing didn't yield useful data, try TRX file
            if summary.total == 0 && summary.failed_tests.is_empty() {
                if let Some(ref trx) = trx_path {
                    if let Some(trx_summary) = binlog::parse_trx_file(trx) {
                        summary = trx_summary;
                    }
                }
            }

            let summary = normalize_test_summary(summary, output.status.success());
            format_test_output(&summary, &binlog_path)
        }
        "restore" => {
            let summary = binlog::parse_restore(&binlog_path, &raw)?;
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
}
