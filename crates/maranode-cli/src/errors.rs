use colored::Colorize;

pub fn print_cli_error(err: &anyhow::Error) {
    let msg = err.to_string();

    eprintln!("{} {}", "error:".red().bold(), msg);

    let mut source = err.source();
    while let Some(cause) = source {
        eprintln!("       {} {}", "→".dimmed(), cause.to_string().dimmed());
        source = cause.source();
    }

    if let Some(hint) = hint_for(&msg) {
        eprintln!("\n  {} {}", "hint:".cyan().bold(), hint);
    }
}

fn hint_for(msg: &str) -> Option<String> {
    let m = msg.to_lowercase();

    if m.contains("connection refused")
        || m.contains("cannot reach")
        || m.contains("could not reach")
        || m.contains("os error 61")
        || m.contains("os error 111")
    {
        return Some(format!(
            "Is the daemon running?  Start it with {} or check with {}",
            "maranode serve".cyan(),
            "maranode status".cyan(),
        ));
    }

    if m.contains("401") || m.contains("unauthorized") {
        return Some(format!(
            "Authentication required. Set {} before running this command.",
            "MARANODE_ADMIN_KEY=<key>".cyan(),
        ));
    }

    if m.contains("403") || m.contains("forbidden") {
        return Some(format!(
            "Access denied. Check that {} matches the daemon's {}.",
            "MARANODE_ADMIN_KEY".cyan(),
            "auth.admin_key".cyan(),
        ));
    }

    if m.contains("404") && (m.contains("model") || m.contains("not found")) {
        return Some(format!(
            "Model not found. List available models with {}",
            "maranode model list".cyan(),
        ));
    }

    if m.contains("rag is not enabled") || m.contains("rag not enabled") {
        return Some(format!(
            "RAG is disabled on the daemon. Restart with {}",
            "maranode serve -- --rag".cyan(),
        ));
    }

    if m.contains("name:tag") || m.contains("invalid model id") {
        return Some(format!(
            "Model IDs use the format {}  e.g. {}",
            "name:tag".cyan(),
            "llama3.2:3b".cyan(),
        ));
    }

    if m.contains("timed out") || m.contains("deadline") {
        return Some(format!(
            "The daemon took too long to respond. Check {}",
            "maranode status".cyan(),
        ));
    }

    None
}

/// return closest option for input, or None if no option within 2 edits
/// matching is case-insensitive
pub fn did_you_mean<'a>(input: &str, options: &[&'a str]) -> Option<&'a str> {
    let lower = input.to_lowercase();

    // exact match ignoring case. return canonical option string
    if let Some(&opt) = options.iter().find(|&&o| o.to_lowercase() == lower) {
        return Some(opt);
    }

    // pick closest by edit distance. max distance is 2
    options
        .iter()
        .map(|&o| (o, edit_distance(&lower, &o.to_lowercase())))
        .filter(|(_, d)| *d <= 2)
        .min_by_key(|(_, d)| *d)
        .map(|(o, _)| o)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());

    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1]
            } else {
                1 + dp[i - 1][j - 1].min(dp[i - 1][j]).min(dp[i][j - 1])
            };
        }
    }
    dp[m][n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_canonical() {
        assert_eq!(
            did_you_mean("GDPR", &["gdpr", "hipaa", "soc2"]),
            Some("gdpr")
        );
    }

    #[test]
    fn close_typo() {
        assert_eq!(
            did_you_mean("gpdr", &["gdpr", "hipaa", "soc2"]),
            Some("gdpr")
        );
        assert_eq!(
            did_you_mean("soc-2", &["gdpr", "hipaa", "soc2", "iso27001"]),
            Some("soc2")
        );
    }

    #[test]
    fn no_suggestion_for_garbage() {
        assert_eq!(did_you_mean("zzzzz", &["gdpr", "hipaa"]), None);
    }

    #[test]
    fn edit_distance_basic() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", "abc"), 0);
    }
}
