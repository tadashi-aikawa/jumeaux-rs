//! Python addons/final/summary.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub sysout: bool,
}

pub fn render(r: &crate::models::Report) -> String {
    let description = r
        .description
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("\n{s}\n"))
        .unwrap_or_default();
    let s = &r.summary;
    format!(
        "\n===================================================================\n| {}\n===================================================================\n{}\n-------------------------------------------------------------------\n| key   | {}\n| One   | {} ({})\n| Other | {} ({})\n-------------------------------------------------------------------\n\n-------------------------------------------------------------------\n|        Same         |      Differenct     |       Failure       |\n-------------------------------------------------------------------\n|{:^21}|{:^21}|{:^21}|\n-------------------------------------------------------------------\n\n-------------------------------------------------------------------\n| Threads         | {}\n| Processes       | {}\n| Begin           | {}\n| End             | {}\n| Elapsed seconds | {}\n-------------------------------------------------------------------\n\n\n>>> By Jumeaux {}\n",
        r.title,
        description,
        r.key,
        s.one.host,
        s.one.name,
        s.other.host,
        s.other.name,
        s.status.same,
        s.status.different,
        s.status.failure,
        s.concurrency.threads,
        s.concurrency.processes,
        s.time.start,
        s.time.end,
        s.time.elapsed_sec,
        r.version
    )
}
