//! a small, dependency-free scan for common insecure code patterns. it is a
//! heuristic hint for review, not a replacement for a real SAST tool, so it aims
//! for high-signal rules over completeness.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    High,
    Medium,
    Low,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub rule: &'static str,
    pub title: &'static str,
    pub severity: Severity,
    /// 1-based line number
    pub line: usize,
    pub snippet: String,
}

struct Rule {
    id: &'static str,
    title: &'static str,
    severity: Severity,
    test: fn(&str, &str) -> bool,
}

const RULES: &[Rule] = &[
    Rule {
        id: "hardcoded-secret",
        title: "possible hard-coded credential",
        severity: Severity::High,
        test: rule_secret,
    },
    Rule {
        id: "weak-crypto",
        title: "weak hash algorithm (MD5/SHA-1)",
        severity: Severity::Medium,
        test: rule_weak_crypto,
    },
    Rule {
        id: "sql-injection",
        title: "SQL built by string concatenation",
        severity: Severity::High,
        test: rule_sql,
    },
    Rule {
        id: "dangerous-exec",
        title: "dynamic code or shell execution",
        severity: Severity::High,
        test: rule_exec,
    },
    Rule {
        id: "tls-verification-disabled",
        title: "TLS certificate verification disabled",
        severity: Severity::High,
        test: rule_tls,
    },
    Rule {
        id: "unsafe-deserialization",
        title: "unsafe deserialization",
        severity: Severity::Medium,
        test: rule_deser,
    },
];

pub fn scan(text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_lowercase();
        for rule in RULES {
            if (rule.test)(line, &lower) {
                out.push(Finding {
                    rule: rule.id,
                    title: rule.title,
                    severity: rule.severity,
                    line: i + 1,
                    snippet: snippet(line),
                });
            }
        }
    }
    out
}

/// count findings by severity, returned as (high, medium, low).
pub fn severity_counts(findings: &[Finding]) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for f in findings {
        match f.severity {
            Severity::High => c.0 += 1,
            Severity::Medium => c.1 += 1,
            Severity::Low => c.2 += 1,
        }
    }
    c
}

fn snippet(line: &str) -> String {
    const MAX: usize = 160;
    if line.chars().count() <= MAX {
        line.to_string()
    } else {
        let s: String = line.chars().take(MAX).collect();
        format!("{s}…")
    }
}

/// length of the contents of the first single- or double-quoted string on the line.
fn string_literal_len(line: &str) -> usize {
    let chars: Vec<char> = line.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' || c == '\'' {
            if let Some(rel) = chars[i + 1..].iter().position(|&x| x == c) {
                return rel;
            }
        }
    }
    0
}

/// AWS access key ids look like AKIA followed by 16 upper-case / digit chars.
fn has_aws_key(line: &str) -> bool {
    if let Some(pos) = line.find("AKIA") {
        let run = line[pos + 4..]
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            .count();
        return run >= 16;
    }
    false
}

/// `name(` appearing as a bare call, i.e. not `obj.name(` or `xname(`.
fn bare_call(lower: &str, name: &str) -> bool {
    let pat = format!("{name}(");
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&pat) {
        let idx = from + rel;
        let prev = lower[..idx].chars().next_back();
        let bare = match prev {
            None => true,
            Some(c) => !(c.is_alphanumeric() || c == '_' || c == '.'),
        };
        if bare {
            return true;
        }
        from = idx + pat.len();
    }
    false
}

fn rule_secret(raw: &str, lower: &str) -> bool {
    if lower.contains("-----begin") && lower.contains("private key") {
        return true;
    }
    if has_aws_key(raw) {
        return true;
    }
    const NAMES: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "api_key",
        "apikey",
        "access_key",
        "secret_key",
        "client_secret",
        "private_key",
    ];
    let named = NAMES.iter().any(|n| lower.contains(n));
    let assigned = lower.contains('=') || lower.contains(':');
    named && assigned && string_literal_len(raw) >= 6
}

fn rule_weak_crypto(_raw: &str, lower: &str) -> bool {
    lower.contains("md5") || lower.contains("sha1")
}

fn rule_sql(raw: &str, lower: &str) -> bool {
    let has_sql = lower.contains("select ")
        || lower.contains("insert ")
        || lower.contains("update ")
        || lower.contains("delete ");
    if !has_sql {
        return false;
    }
    // string concatenation or interpolation, not parameter placeholders
    raw.contains("\" +")
        || raw.contains("+ \"")
        || raw.contains("' +")
        || raw.contains("+ '")
        || lower.contains(".format(")
        || lower.contains("format!(")
        || raw.contains("f\"")
        || raw.contains("f'")
        || raw.contains("${")
}

fn rule_exec(_raw: &str, lower: &str) -> bool {
    bare_call(lower, "eval")
        || bare_call(lower, "exec")
        || lower.contains("os.system(")
        || (lower.contains("subprocess") && lower.replace(' ', "").contains("shell=true"))
        || lower.contains("child_process.exec(")
        || lower.replace(' ', "").contains("runtime.getruntime().exec(")
}

fn rule_tls(_raw: &str, lower: &str) -> bool {
    let l = lower.replace(' ', "");
    l.contains("verify=false")
        || l.contains("rejectunauthorized:false")
        || l.contains("insecureskipverify:true")
        || l.contains("danger_accept_invalid_certs(true")
        || l.contains("check_hostname=false")
        || l.contains("cert_none")
}

fn rule_deser(_raw: &str, lower: &str) -> bool {
    lower.contains("pickle.loads(")
        || lower.contains("cpickle.loads(")
        || lower.contains("marshal.load(")
        || (lower.contains("yaml.load(")
            && !lower.contains("safeloader")
            && !lower.contains("safe_load"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules_hit(text: &str) -> Vec<&'static str> {
        scan(text).into_iter().map(|f| f.rule).collect()
    }

    #[test]
    fn flags_hardcoded_password_and_keys() {
        assert!(rules_hit("password = \"hunter2!!\"").contains(&"hardcoded-secret"));
        assert!(rules_hit("let api_key = \"AKIAIOSFODNN7EXAMPLE\";").contains(&"hardcoded-secret"));
        assert!(rules_hit("-----BEGIN RSA PRIVATE KEY-----").contains(&"hardcoded-secret"));
    }

    #[test]
    fn ignores_non_secret_assignments() {
        let hits = rules_hit("password = get_password_from_vault()");
        assert!(!hits.contains(&"hardcoded-secret"));
        assert!(!rules_hit("let count = \"5\";").contains(&"hardcoded-secret"));
    }

    #[test]
    fn flags_weak_crypto() {
        assert!(rules_hit("h = hashlib.md5(data).hexdigest()").contains(&"weak-crypto"));
        assert!(!rules_hit("h = sha256(data)").contains(&"weak-crypto"));
    }

    #[test]
    fn flags_sql_concatenation_not_parameters() {
        assert!(rules_hit("q = \"SELECT * FROM t WHERE id = \" + uid").contains(&"sql-injection"));
        assert!(rules_hit("db.query(f\"SELECT * FROM t WHERE id = {uid}\")").contains(&"sql-injection"));
        // parameterized query is safe
        assert!(!rules_hit("cur.execute(\"SELECT * FROM t WHERE id = %s\", (uid,))")
            .contains(&"sql-injection"));
    }

    #[test]
    fn flags_dangerous_exec_but_not_method_exec() {
        assert!(rules_hit("os.system(cmd)").contains(&"dangerous-exec"));
        assert!(rules_hit("result = eval(user_input)").contains(&"dangerous-exec"));
        assert!(rules_hit("subprocess.run(cmd, shell=True)").contains(&"dangerous-exec"));
        // regex .exec in JS is not a shell call
        assert!(!rules_hit("const m = pattern.exec(line);").contains(&"dangerous-exec"));
    }

    #[test]
    fn flags_disabled_tls_only_when_off() {
        assert!(rules_hit("r = requests.get(url, verify=False)").contains(&"tls-verification-disabled"));
        assert!(rules_hit("tls.Config{ InsecureSkipVerify: true }").contains(&"tls-verification-disabled"));
        assert!(!rules_hit("r = requests.get(url, verify=True)").contains(&"tls-verification-disabled"));
    }

    #[test]
    fn flags_unsafe_deserialization() {
        assert!(rules_hit("obj = pickle.loads(blob)").contains(&"unsafe-deserialization"));
        assert!(rules_hit("cfg = yaml.load(f)").contains(&"unsafe-deserialization"));
        assert!(!rules_hit("cfg = yaml.safe_load(f)").contains(&"unsafe-deserialization"));
    }

    #[test]
    fn reports_line_numbers_and_counts() {
        let src = "ok = 1\npassword = \"s3cr3t-value\"\nx = 2\nos.system(cmd)\n";
        let findings = scan(src);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].line, 2);
        assert_eq!(findings[1].line, 4);
        let (high, _med, _low) = severity_counts(&findings);
        assert_eq!(high, 2);
    }
}
