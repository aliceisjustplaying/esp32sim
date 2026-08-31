use std::fs;
use std::path::Path;

fn validate_workflow(name: &str, text: &str) -> Vec<String> {
    let mut errors = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(reference) = trimmed
            .strip_prefix("- uses: ")
            .or_else(|| trimmed.strip_prefix("uses: "))
        {
            if reference.starts_with("./") {
                continue;
            }
            let Some((_, revision)) = reference.split_once('@') else {
                errors.push(format!("{name}:{}: action has no revision", index + 1));
                continue;
            };
            let revision = revision.split_whitespace().next().unwrap_or("");
            if revision.len() != 40 || !revision.chars().all(|c| c.is_ascii_hexdigit()) {
                errors.push(format!(
                    "{name}:{}: action revision is not a full commit SHA",
                    index + 1
                ));
            }
        }
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut start = 0;
    while start < lines.len() {
        let indent = lines[start].len() - lines[start].trim_start().len();
        if indent == 6 && lines[start].trim_start().starts_with("- ") {
            let mut end = start + 1;
            while end < lines.len() {
                let next_indent = lines[end].len() - lines[end].trim_start().len();
                if next_indent == 6 && lines[end].trim_start().starts_with("- ") {
                    break;
                }
                end += 1;
            }
            let step = lines[start..end].join("\n");
            let downloads =
                step.contains("curl ") || step.contains("curl -") || step.contains("wget ");
            let verifies_digest = step.contains("sha256sum --check")
                || step.contains("sha256sum -c")
                || step.contains("shasum -a 256 -c");
            if downloads && !verifies_digest {
                errors.push(format!(
                    "{name}:{}: network download step has no SHA-256 verification",
                    start + 1
                ));
            }
            start = end;
        } else {
            start += 1;
        }
    }
    errors
}

fn self_test() {
    let mutable_action = "jobs:\n  x:\n    steps:\n      - uses: actions/checkout@v4\n";
    assert!(!validate_workflow("mutable.yml", mutable_action).is_empty());
    let unverified_download = "jobs:\n  x:\n    steps:\n      - name: fetch\n        run: curl -fsSL https://example.invalid/a -o a\n";
    assert!(!validate_workflow("download.yml", unverified_download).is_empty());
    let verified = "jobs:\n  x:\n    steps:\n      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262\n      - name: fetch\n        run: |\n          curl -fsSL https://example.invalid/a -o a\n          echo 'abc  a' | sha256sum --check\n";
    assert!(validate_workflow("verified.yml", verified).is_empty());
    println!("CI policy self-test: 3 boundary cases passed");
}

fn main() {
    if std::env::args().any(|arg| arg == "--self-test") {
        self_test();
        return;
    }
    let workflow_dir = Path::new(".github/workflows");
    let mut paths: Vec<_> = fs::read_dir(workflow_dir)
        .expect("read .github/workflows")
        .map(|entry| entry.expect("read workflow entry").path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("yml" | "yaml")
            )
        })
        .collect();
    paths.sort();

    let mut errors = Vec::new();
    for path in &paths {
        let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        errors.extend(validate_workflow(&path.display().to_string(), &text));
    }
    if !errors.is_empty() {
        for error in errors {
            eprintln!("{error}");
        }
        std::process::exit(1);
    }
    println!(
        "CI policy: {} workflow(s), all actions immutable, all explicit downloads digest-verified",
        paths.len()
    );
}
