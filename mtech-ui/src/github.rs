//! GitHub issue creation via the Cloudflare GitHub proxy. Shared by `displays`
//! (MasterTech) and `qc_app` so both file bug reports the same way.

use std::str::FromStr;

use reqwest::{
    header::{HeaderName, ACCEPT, USER_AGENT},
    Client,
};

/// Cloudflare Worker in front of the GitHub API — CORS-safe for browser WASM.
const GIT_MASTER_TECH_REPO_BASE: &str =
    "https://git.master-tech.app/repos/shadowbrok3r/MastertechProject";

/// GitHub issue `body` max length (characters).
pub const GITHUB_ISSUE_BODY_CHAR_LIMIT: usize = 65_536;

/// Space reserved for log lines after worst-case description trim.
const GITHUB_ISSUE_MIN_LOG_CHARS: usize = 4_096;

#[inline]
fn truncate_issue_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// User description + metadata + collapsible logs, trimmed to GitHub's body limit.
#[must_use]
pub fn build_github_issue_body(
    user_description: &str,
    user_name: &str,
    user_email: &str,
    logs: &str,
) -> String {
    let mid = format!(
        "\n\n**User:** {} - {}\n\n<details>\n<summary>Application Logs (last 50 entries)</summary>\n\n```\n",
        user_name, user_email
    );
    let end = "\n```\n</details>";

    let mid_n = mid.chars().count();
    let end_n = end.chars().count();

    let mut desc = user_description.to_string();
    let max_desc =
        GITHUB_ISSUE_BODY_CHAR_LIMIT.saturating_sub(mid_n + end_n + GITHUB_ISSUE_MIN_LOG_CHARS);
    if desc.chars().count() > max_desc {
        let note = "\n\n_(Description truncated: GitHub issue body limit is 65536 characters.)_";
        let budget = max_desc.saturating_sub(note.chars().count());
        desc = truncate_issue_chars(&desc, budget);
        desc.push_str(note);
    }

    let max_logs =
        GITHUB_ISSUE_BODY_CHAR_LIMIT.saturating_sub(desc.chars().count() + mid_n + end_n);
    let logs_part = if logs.chars().count() > max_logs {
        let note = "\n… _(logs truncated: GitHub issue body limit)_";
        let budget = max_logs.saturating_sub(note.chars().count());
        format!("{}{}", truncate_issue_chars(logs, budget.max(1)), note)
    } else {
        logs.to_string()
    };

    format!("{}{}{}{}", desc, mid, logs_part, end)
}

/// Create an issue via the GitHub proxy (no auth token — public repo).
pub async fn create_new_issue(
    title: String,
    body: String,
    client: Client,
) -> anyhow::Result<String> {
    let params = serde_json::json!({
        "title": title,
        "body": body,
        "assignees": ["shadowbrok3r"],
        "labels": ["bug"],
    });
    let res = client
        .post(format!("{GIT_MASTER_TECH_REPO_BASE}/issues"))
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, "MtechServer")
        .header(HeaderName::from_str("X-GitHub-Api-Version").unwrap(), "2022-11-28")
        .json(&params)
        .send()
        .await?
        .text()
        .await?;

    Ok(res)
}
