use jumeaux::models::Report;

/// Python版の保存済みfixtureを使うretry試験では、利用者と同じ移行を行う。
pub fn without_viewer(report: &Report) -> Report {
    let mut report = report.clone();
    if let Some(addons) = &mut report.addons {
        addons.final_addons.retain(|addon| addon.name != "viewer");
    }
    report
}
