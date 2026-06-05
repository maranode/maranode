use anyhow::{Context, Result};

const NS_PREFIX: &str = "maranode-ws-";

pub fn ns_name(slug: &str) -> String {
    format!("{}{}", NS_PREFIX, slug)
}

pub fn create(slug: &str) -> Result<()> {
    let name = ns_name(slug);
    let status = std::process::Command::new("ip")
        .args(["netns", "add", &name])
        .status()
        .context("running ip netns add")?;
    if !status.success() {
        anyhow::bail!("ip netns add {} failed (status {})", name, status);
    }
    Ok(())
}

pub fn delete(slug: &str) -> Result<()> {
    let name = ns_name(slug);
    let status = std::process::Command::new("ip")
        .args(["netns", "del", &name])
        .status()
        .context("running ip netns del")?;
    if !status.success() {
        anyhow::bail!("ip netns del {} failed (status {})", name, status);
    }
    Ok(())
}

pub fn exists(slug: &str) -> bool {
    std::path::Path::new(&format!("/var/run/netns/{}{}", NS_PREFIX, slug)).exists()
}

pub fn list() -> Result<Vec<String>> {
    let out = std::process::Command::new("ip")
        .args(["netns", "list"])
        .output()
        .context("running ip netns list")?;
    let text = String::from_utf8_lossy(&out.stdout);
    let names = text
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| n.starts_with(NS_PREFIX))
        .map(|n| n[NS_PREFIX.len()..].to_string())
        .collect();
    Ok(names)
}
