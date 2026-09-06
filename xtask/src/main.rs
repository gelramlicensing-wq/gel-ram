#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const ALLOWED_EXTENSIONS: &[&str] = &["rs", "md", "toml", "yml", "txt", "gel"];
const ALLOWED_EXTENSIONLESS: &[&str] = &[
    "Cargo.lock",
    "LICENSE",
    "NOTICE",
    ".gitignore",
    ".gitattributes",
];

/// A backtick-quoted token with one of these extensions is a repository path citation.
const REFERENCE_EXTENSIONS: &[&str] = &["md", "toml", "txt", "yml", "rs", "lock"];
/// A backtick-quoted token starting with one of these prefixes is a repository path citation.
const REFERENCE_PREFIXES: &[&str] = &["docs/", ".github/", "crates/", "xtask/"];

/// The ticked CLA acknowledgement line from `.github/PULL_REQUEST_TEMPLATE.md`.
const CLA_ACK_TICKED: &[&str] = &[
    "[x] I have completed the GEL RAM CLA privately with the project before opening this pull request.",
    "[X] I have completed the GEL RAM CLA privately with the project before opening this pull request.",
];

const USAGE: &str =
    "verify|rust-only|licensing|ci-policy|docs-refs|cla-ack|fmt|clippy|test|bench|physics";
const CHECKOUT_SHA: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const PROJECT_EMAIL: &str = "gelram.licensing@gmail.com";

// Distribution regression fingerprint, NOT a cryptographic authenticity check.
fn canonical_license(bytes: &[u8]) -> bool {
    bytes.len() == 4563 && gel_core::crc64_ecma(bytes) == 0x69c6_11a1_8474_b05b
}

// Narrow accidental-contact check, NOT a general secret or email scanner.
// A bare domain suffix in source code is not an email address.
fn has_nonproject_gmail(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || ".+-_@".contains(c)))
        .any(|token| {
            let token = token.trim_matches('.');
            let Some((local, domain)) = token.rsplit_once('@') else {
                return false;
            };
            !local.is_empty()
                && domain.eq_ignore_ascii_case("gmail.com")
                && !token.eq_ignore_ascii_case(PROJECT_EMAIL)
        })
}

fn project_contact_privacy(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    walk(root, &mut files).map_err(|e| e.to_string())?;
    for path in files {
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Ok(text) = std::str::from_utf8(&bytes) {
            if has_nonproject_gmail(text) {
                // Do not echo a potentially private address into public CI logs.
                return Err(format!("non-project Gmail address in {}", path.display()));
            }
        }
    }
    println!("PROJECT_CONTACT_GATE=PASS");
    Ok(())
}
const FMT_CHECK_ARGS: &[&str] = &["fmt", "--all", "--", "--check"];
const CLIPPY_ARGS: &[&str] = &[
    "clippy",
    "--locked",
    "--offline",
    "--workspace",
    "--all-targets",
    "--",
    "-D",
    "warnings",
];
const TEST_ARGS: &[&str] = &[
    "test",
    "--locked",
    "--offline",
    "--workspace",
    "--all-targets",
];

fn walk(root: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x == "target" || x == ".git")
        {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            out.push(path);
        } else if metadata.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn workspace_root() -> Result<&'static Path, String> {
    // Resolve the source checkout embedded by this build, not the caller's cwd.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "missing workspace root".into())
}

fn rust_only_at(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    walk(root, &mut files).map_err(|e| e.to_string())?;
    let mut bad = Vec::new();
    for path in files {
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            bad.push(format!("symlink is not allowed: {}", path.display()));
            continue;
        }
        #[cfg(unix)]
        if metadata.permissions().mode() & 0o111 != 0 {
            bad.push(format!(
                "executable file is not allowed in the release tree: {}",
                path.display()
            ));
            continue;
        }
        let allowed = path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|ext| ALLOWED_EXTENSIONS.contains(&ext))
            || path
                .file_name()
                .and_then(|x| x.to_str())
                .is_some_and(|name| ALLOWED_EXTENSIONLESS.contains(&name));
        if !allowed {
            bad.push(format!(
                "file type is outside the release allow-list: {}",
                path.display()
            ));
        }
    }
    bad.sort();
    bad.dedup();
    if bad.is_empty() {
        println!("RUST_ONLY_GATE=PASS");
        Ok(())
    } else {
        for message in bad {
            eprintln!("{message}");
        }
        Err("RUST_ONLY_GATE=FAIL".into())
    }
}

fn rust_only() -> Result<(), String> {
    rust_only_at(workspace_root()?)
}

fn require(text: &str, needle: &str, file: &str) -> Result<(), String> {
    if text.contains(needle) {
        Ok(())
    } else {
        Err(format!("{file} is missing required text: {needle}"))
    }
}

fn licensing() -> Result<(), String> {
    let root = workspace_root()?;
    let read = |name: &str| fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"));
    let mode = read("LICENSE-MODE.txt")?;
    let license = read("LICENSE")?;
    let cargo = read("Cargo.toml")?;
    let notice = read("NOTICE")?;
    let commercial = read("COMMERCIAL-LICENSE.md")?;
    let cla = read("CLA.md")?;
    if !canonical_license(license.as_bytes()) {
        return Err("LICENSE differs from the preserved PolyForm distribution bytes".into());
    }
    match mode.trim() {
        "PolyForm-Noncommercial-1.0.0 + Commercial + CLA" => {
            require(
                &license,
                "# PolyForm Noncommercial License 1.0.0",
                "LICENSE",
            )?;
            require(&license, "## Noncommercial Purposes", "LICENSE")?;
            require(
                &cargo,
                "license = \"PolyForm-Noncommercial-1.0.0\"",
                "Cargo.toml",
            )?;
            require(&notice, "Required Notice:", "NOTICE")?;
        }
        other => return Err(format!("unsupported LICENSE-MODE.txt value: {other}")),
    }
    require(
        &commercial,
        "not itself a commercial license grant",
        "COMMERCIAL-LICENSE.md",
    )?;
    require(&cla, "complete the CLA privately", "CLA.md")?;
    project_contact_privacy(root)?;
    println!("LICENSING_GATE=PASS");
    println!("LICENSE_MODE={}", mode.trim());
    Ok(())
}

fn ci_policy() -> Result<(), String> {
    let root = workspace_root()?;
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .map_err(|e| format!(".github/workflows/ci.yml: {e}"))?;
    let cla = fs::read_to_string(root.join(".github/workflows/cla.yml"))
        .map_err(|e| format!(".github/workflows/cla.yml: {e}"))?;
    require(&ci, CHECKOUT_SHA, ".github/workflows/ci.yml")?;
    require(
        &ci,
        "persist-credentials: false",
        ".github/workflows/ci.yml",
    )?;
    require(&ci, "contents: read", ".github/workflows/ci.yml")?;
    require(&cla, "pull_request_target:", ".github/workflows/cla.yml")?;
    require(&cla, "permissions: {}", ".github/workflows/cla.yml")?;
    require(
        &cla,
        "github.event.pull_request.author_association",
        ".github/workflows/cla.yml",
    )?;
    require(
        &cla,
        "repository owner is the Project Licensor",
        ".github/workflows/cla.yml",
    )?;
    require(&cla, "grep -Fqx", ".github/workflows/cla.yml")?;
    if cla.contains("actions/checkout") || cla.contains("cargo run") {
        return Err(
            ".github/workflows/cla.yml must not check out or execute pull-request code".into(),
        );
    }
    println!("CI_POLICY_GATE=PASS");
    Ok(())
}

/// Width of a code-fence line (three or more leading backticks), if `line` is one.
fn fence_width(line: &str) -> Option<usize> {
    let width = line.trim_start().bytes().take_while(|b| *b == b'`').count();
    (width >= 3).then_some(width)
}

/// Contents of the inline code spans in one Markdown line.
///
/// A span opens with a run of backticks and closes with the next run of the
/// same width; a run without a closer is literal text and scanning resumes
/// after it. Backticks are ASCII, so every slice boundary is a char boundary.
fn code_spans(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        let width = i - open;
        let start = i;
        let mut close = None;
        while i < bytes.len() {
            if bytes[i] != b'`' {
                i += 1;
                continue;
            }
            let run = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            if i - run == width {
                close = Some(run);
                break;
            }
        }
        match close {
            Some(end) => spans.push(&line[start..end]),
            None => i = start,
        }
    }
    spans
}

/// One space of padding on both sides of a span is not part of its content.
fn strip_span_padding(span: &str) -> &str {
    if span.len() >= 2 && span.starts_with(' ') && span.ends_with(' ') && !span.trim().is_empty() {
        &span[1..span.len() - 1]
    } else {
        span
    }
}

/// Whether a code-span token is shaped like a repository path that must exist.
fn is_reference(token: &str) -> bool {
    if token.is_empty()
        || token
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '<' | '>' | '*' | '{' | '$'))
    {
        return false;
    }
    if ALLOWED_EXTENSIONLESS.contains(&token) {
        return true;
    }
    if REFERENCE_PREFIXES
        .iter()
        .any(|prefix| token.starts_with(prefix))
    {
        return true;
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-'))
    {
        return false;
    }
    token
        .rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && REFERENCE_EXTENSIONS.contains(&ext))
}

/// Repository path citations in a Markdown document as `(line, token)`, lines 1-based.
/// Fenced code blocks are not scanned.
fn reference_tokens(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut fence = None;
    for (index, line) in text.lines().enumerate() {
        if let Some(open) = fence {
            let closes = fence_width(line).is_some_and(|width| width >= open)
                && line.trim().trim_start_matches('`').trim().is_empty();
            if closes {
                fence = None;
            }
            continue;
        }
        if let Some(width) = fence_width(line) {
            fence = Some(width);
            continue;
        }
        for span in code_spans(line) {
            let token = strip_span_padding(span);
            if is_reference(token) {
                found.push((index + 1, token));
            }
        }
    }
    found
}

fn docs_refs() -> Result<(), String> {
    let root = workspace_root()?;
    let mut files = Vec::new();
    walk(root, &mut files).map_err(|e| e.to_string())?;
    files.sort();
    let mut checked = 0usize;
    let mut missing = Vec::new();
    for path in files
        .iter()
        .filter(|path| path.extension().and_then(OsStr::to_str) == Some("md"))
    {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let citing = path.strip_prefix(root).unwrap_or(path.as_path());
        for (line, token) in reference_tokens(&text) {
            checked += 1;
            if !root.join(token).exists() {
                missing.push(format!(
                    "{}:{line}: missing repository reference: {token}",
                    citing.display()
                ));
            }
        }
    }
    if checked == 0 {
        return Err("DOCS_REFS_GATE=FAIL: no repository references found in any .md file".into());
    }
    if missing.is_empty() {
        println!("DOCS_REFS_GATE=PASS");
        println!("DOCS_REFS_CHECKED={checked}");
        Ok(())
    } else {
        for line in &missing {
            eprintln!("{line}");
        }
        Err("DOCS_REFS_GATE=FAIL".into())
    }
}

fn cla_ack() -> Result<(), String> {
    let body = std::env::var_os("PR_BODY")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    if CLA_ACK_TICKED.iter().any(|line| body.contains(line)) {
        println!("CLA_ACK_GATE=PASS");
        Ok(())
    } else {
        eprintln!(
            "PR_BODY is unset or does not contain the ticked CLA acknowledgement line from .github/PULL_REQUEST_TEMPLATE.md."
        );
        eprintln!(
            "A completed CLA must be on file with the project before a pull request is opened; see CLA.md and CONTRIBUTING.md."
        );
        Err("CLA_ACK_GATE=FAIL".into())
    }
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(workspace_root()?)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed: {status}"))
    }
}

fn run_docs() -> Result<(), String> {
    let status = Command::new("cargo")
        .args(["doc", "--locked", "--offline", "--workspace", "--no-deps"])
        .env("RUSTDOCFLAGS", "-D warnings")
        .current_dir(workspace_root()?)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo doc failed: {status}"))
    }
}

/// `cargo run --release -p <package> -- <args>` inside the workspace.
fn run_release_binary<S: AsRef<OsStr>>(package: &str, args: &[S]) -> Result<(), String> {
    let mut command = Command::new("cargo");
    command.args([
        "run",
        "--locked",
        "--offline",
        "--release",
        "-p",
        package,
        "--",
    ]);
    command.args(args);
    let status = command
        .current_dir(workspace_root()?)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{package} failed: {status}"))
    }
}

fn verify() -> Result<(), String> {
    rust_only()?;
    licensing()?;
    ci_policy()?;
    docs_refs()?;
    run("cargo", FMT_CHECK_ARGS)?;
    run("cargo", CLIPPY_ARGS)?;
    run(
        "cargo",
        &["build", "--locked", "--offline", "--release", "--workspace"],
    )?;
    run_docs()?;
    run("cargo", TEST_ARGS)?;
    run_release_binary("gel-cli", &["selftest"])?;
    run_release_binary("gel-bench", &["8192", "3", "2"])?;
    run_release_binary("gel-bench", &["8192", "3", "1"])?;
    println!("GEL_VERIFY_ALL=PASS");
    Ok(())
}

fn collect_args() -> Result<Vec<String>, String> {
    std::env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string()
                .map_err(|bad| format!("argument is not valid UTF-8: {}", bad.to_string_lossy()))
        })
        .collect()
}

fn dispatch(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("verify") => verify(),
        Some("rust-only") => rust_only(),
        Some("licensing") => licensing(),
        Some("ci-policy") => ci_policy(),
        Some("docs-refs") => docs_refs(),
        Some("cla-ack") => cla_ack(),
        Some("fmt") => run("cargo", FMT_CHECK_ARGS),
        Some("clippy") => run("cargo", CLIPPY_ARGS),
        Some("test") => run("cargo", TEST_ARGS),
        Some("bench") => run_release_binary("gel-bench", &args[1..]),
        Some("physics") => run_release_binary("gel-physics", &args[1..]),
        Some(x) => Err(format!("unknown xtask: {x}; use {USAGE}")),
    }
}

fn main() -> ExitCode {
    match collect_args().and_then(|args| dispatch(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_license_rejects_changed_or_appended_bytes() {
        let original = include_bytes!("../../LICENSE");
        assert!(canonical_license(original));
        let mut changed = original.to_vec();
        changed[0] ^= 1;
        assert!(!canonical_license(&changed));
        changed = original.to_vec();
        changed.push(b'\n');
        assert!(!canonical_license(&changed));
    }

    #[test]
    fn contact_gate_distinguishes_source_suffix_from_an_address() {
        assert!(!has_nonproject_gmail("token.ends_with(\"@gmail.com\")"));
        assert!(!has_nonproject_gmail(PROJECT_EMAIL));
        assert!(!has_nonproject_gmail(&format!("<mailto:{PROJECT_EMAIL}>.")));
        assert!(!has_nonproject_gmail(&PROJECT_EMAIL.to_uppercase()));
        for local in ["fixture", "test.user", "test+tag", "test_user"] {
            assert!(has_nonproject_gmail(&format!("`{local}@{}.`", "gmail.com")));
        }
    }

    #[cfg(unix)]
    #[test]
    fn rust_only_gate_rejects_symlinks_and_executable_files() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("gel-rust-only-gate-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("good.rs");
        fs::write(&source, "fn main() {}\n").unwrap();
        assert!(rust_only_at(&root).is_ok());

        let link = root.join("link.rs");
        symlink("good.rs", &link).unwrap();
        let error = rust_only_at(&root).unwrap_err();
        assert_eq!(error, "RUST_ONLY_GATE=FAIL");
        fs::remove_file(&link).unwrap();

        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
        let error = rust_only_at(&root).unwrap_err();
        assert_eq!(error, "RUST_ONLY_GATE=FAIL");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn code_spans_follow_backtick_run_width() {
        assert_eq!(
            code_spans("see `a.md` and ``b `x` c`` here"),
            vec!["a.md", "b `x` c"]
        );
        assert_eq!(code_spans("`` unmatched ` one.md `"), vec![" one.md "]);
        assert_eq!(code_spans("no spans ` here"), Vec::<&str>::new());
        assert_eq!(strip_span_padding(" one.md "), "one.md");
        assert_eq!(strip_span_padding("  "), "  ");
    }

    #[test]
    fn fenced_blocks_are_not_scanned() {
        let text = "```text\n`docs/inside.md`\n```\n`docs/outside.md`\n````\n```\n`docs/still-inside.md`\n````\n`CLA.md`\n";
        assert_eq!(
            reference_tokens(text),
            vec![(4, "docs/outside.md"), (9, "CLA.md")]
        );
    }

    #[test]
    fn reference_shape_matches_repository_paths_only() {
        for yes in [
            "CLA.md",
            "docs/",
            ".github/workflows/ci.yml",
            "crates/gel-core/src/lib.rs",
            "xtask/Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "LICENSE-MODE.txt",
            "LICENSE",
            "NOTICE",
            ".gitignore",
        ] {
            assert!(is_reference(yes), "{yes}");
        }
        for no in [
            "",
            ".gel",
            "xtask",
            "gel-bench",
            "u64::count_ones()",
            "gelram.licensing@gmail.com",
            "<ORB_COUNT> <ROUNDS> [THREADS]",
            "docs/<name>.md",
            "record_count * 128",
            "GEL_BENCH_V3",
            "unsafe_code = \"forbid\"",
            "$HOME/x.md",
            "{root}/x.md",
            "x86_64-unknown-linux-gnu",
        ] {
            assert!(!is_reference(no), "{no}");
        }
    }

    #[test]
    fn cla_ack_needles_match_the_template_line() {
        for needle in CLA_ACK_TICKED {
            assert!(needle.starts_with("[x] ") || needle.starts_with("[X] "));
            assert!(needle.ends_with(" before opening this pull request."));
        }
        assert_eq!(CLA_ACK_TICKED[0][4..], CLA_ACK_TICKED[1][4..]);
    }
}
