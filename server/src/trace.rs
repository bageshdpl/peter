//! Traceability: reads requirements (URS-n), specification items (FS-n, CS-n)
//! and tests (TS-n, IQ-n) out of a session's documents and links them. Plain
//! rules, no AI, so the same documents always give the same matrix.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::export::markdown::{Block, Inline, parse, plain};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Requirement {
    pub id: String,
    pub text: String,
    pub category: String,
    pub gxp_critical: String,
    pub risk: String,
    pub evidence: Vec<String>,
    /// Every cell of the row, for finding the controls it names.
    #[serde(skip)]
    pub row_text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpecItem {
    pub id: String,
    pub step: String,
    pub text: String,
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    NotExecuted,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Test {
    pub id: String,
    pub step: String,
    pub title: String,
    pub requirements: Vec<String>,
    pub outcome: Outcome,
    pub steps_total: usize,
    pub steps_passed: usize,
    pub steps_failed: usize,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    Failed,
    NotExecuted,
    NotTested,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub requirement: Requirement,
    pub specs: Vec<String>,
    pub tests: Vec<String>,
    pub status: Status,
    pub high_risk: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub requirements: usize,
    pub passed: usize,
    pub failed: usize,
    pub not_executed: usize,
    pub not_tested: usize,
    pub high_risk_open: usize,
    pub tests: usize,
}

#[derive(Debug, Serialize)]
pub struct Matrix {
    pub rows: Vec<Row>,
    pub tests: Vec<Test>,
    pub specs: Vec<SpecItem>,
    pub summary: Summary,
    pub warnings: Vec<String>,
}

/// Every ID with the given prefix in a text, in order of first appearance.
/// "URS-001", "URS-1" and "URS-0001" are kept as written.
pub fn ids(text: &str, prefix: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let pat = format!("{prefix}-");
    let mut rest = text;
    while let Some(pos) = rest.find(&pat) {
        let before_ok = rest[..pos].chars().last().is_none_or(|c| !c.is_alphanumeric());
        let after = &rest[pos + pat.len()..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if before_ok && !digits.is_empty() {
            let id = format!("{pat}{digits}");
            if !found.contains(&id) {
                found.push(id);
            }
        }
        rest = &rest[pos + pat.len()..];
    }
    found
}

/// The ID a cell starts with, if any, for the given prefixes.
fn leading_id(cell: &str, prefixes: &[&str]) -> Option<String> {
    let cell = cell.trim().trim_start_matches(['*', '_', '`']);
    prefixes.iter().find_map(|p| {
        let found = ids(cell, p);
        found.first().filter(|id| cell.starts_with(id.as_str())).cloned()
    })
}

fn cites(inlines: &[Inline], out: &mut BTreeSet<String>) {
    for i in inlines {
        match i {
            Inline::Cite(id) if !id.starts_with("D:") => {
                out.insert(id.clone());
            }
            Inline::Bold(c) | Inline::Italic(c) => cites(c, out),
            _ => {}
        }
    }
}

fn block_cites(block: &Block, out: &mut BTreeSet<String>) {
    match block {
        Block::Heading(_, i) | Block::Para(i) | Block::Quote(i) => cites(i, out),
        Block::List { items, .. } => items.iter().for_each(|i| cites(i, out)),
        Block::Table { head, rows } => {
            head.iter().for_each(|c| cites(c, out));
            rows.iter().flatten().for_each(|c| cites(c, out));
        }
        Block::Rule => {}
    }
}

fn block_text(block: &Block) -> String {
    match block {
        Block::Heading(_, i) | Block::Para(i) | Block::Quote(i) => plain(i),
        Block::List { items, .. } => items.iter().map(|i| plain(i)).collect::<Vec<_>>().join("\n"),
        Block::Table { head, rows } => {
            let mut t: Vec<String> = head.iter().map(|c| plain(c)).collect();
            t.extend(rows.iter().flatten().map(|c| plain(c)));
            t.join(" | ")
        }
        Block::Rule => String::new(),
    }
}

/// Finds a column whose header matches, preferring exact names.
fn column(head: &[String], exact: &[&str], contains: &[&str]) -> Option<usize> {
    let lower: Vec<String> = head.iter().map(|h| h.trim().to_lowercase()).collect();
    lower
        .iter()
        .position(|h| exact.contains(&h.as_str()))
        .or_else(|| lower.iter().position(|h| contains.iter().any(|c| h.contains(c))))
}

pub fn requirements(body: &str) -> Vec<Requirement> {
    let mut out = Vec::new();
    for block in parse(body) {
        let Block::Table { head, rows } = block else { continue };
        let names: Vec<String> = head.iter().map(|h| plain(h)).collect();
        let id_col = column(&names, &["id", "ref", "requirement id"], &[]).unwrap_or(0);
        let Some(text_col) = column(&names, &["requirement", "requirement text", "description"], &["requirement", "shall"]) else { continue };
        if text_col == id_col {
            continue;
        }
        let cat = column(&names, &["category", "type"], &["category"]);
        let gxp = column(&names, &[], &["gxp"]);
        let risk = column(&names, &["risk", "risk level", "overall risk"], &["risk"]);
        for row in rows {
            let cell = |i: Option<usize>| i.and_then(|i| row.get(i)).map(|c| plain(c).trim().to_string()).unwrap_or_default();
            let Some(id) = leading_id(&cell(Some(id_col)), &["URS"]) else { continue };
            let mut ev = BTreeSet::new();
            row.iter().for_each(|c| cites(c, &mut ev));
            out.push(Requirement {
                id,
                text: cell(Some(text_col)),
                category: cell(cat),
                gxp_critical: cell(gxp),
                risk: cell(risk).to_lowercase(),
                evidence: ev.into_iter().collect(),
                row_text: row.iter().map(|c| plain(c)).collect::<Vec<_>>().join(" | "),
            });
        }
    }
    out
}

pub fn spec_items(body: &str, step: &str) -> Vec<SpecItem> {
    let mut out = Vec::new();
    for block in parse(body) {
        let Block::Table { rows, .. } = block else { continue };
        for row in rows {
            let first = row.first().map(|c| plain(c)).unwrap_or_default();
            let Some(id) = leading_id(&first, &["FS", "CS"]) else { continue };
            let all: Vec<String> = row.iter().map(|c| plain(c)).collect();
            let text = all.iter().skip(1).find(|c| !c.trim().is_empty() && ids(c, "URS").is_empty()).cloned().unwrap_or_default();
            out.push(SpecItem { id, step: step.to_string(), text: text.chars().take(200).collect(), requirements: ids(&all.join(" "), "URS") });
        }
    }
    out
}

fn outcome_of(cells: &[String]) -> (usize, usize, usize) {
    let (mut total, mut passed, mut failed) = (0, 0, 0);
    for c in cells {
        total += 1;
        let v = c.trim().to_lowercase();
        if v.starts_with("fail") {
            failed += 1;
        } else if v.starts_with("pass") {
            passed += 1;
        }
    }
    (total, passed, failed)
}

fn finish(total: usize, passed: usize, failed: usize) -> Outcome {
    if failed > 0 {
        Outcome::Failed
    } else if total > 0 && passed == total {
        Outcome::Passed
    } else {
        Outcome::NotExecuted
    }
}

pub fn tests(body: &str, step: &str) -> Vec<Test> {
    let prefixes = ["TS", "IQ", "OQ", "PQ"];
    let blocks = parse(body);
    let mut out: Vec<Test> = Vec::new();
    let mut current: Option<(usize, Test, Vec<String>)> = None; // (heading level, test, pass/fail cells)

    let close = |cur: Option<(usize, Test, Vec<String>)>, out: &mut Vec<Test>| {
        if let Some((_, mut t, cells)) = cur {
            let (total, passed, failed) = outcome_of(&cells);
            t.steps_total = total;
            t.steps_passed = passed;
            t.steps_failed = failed;
            t.outcome = finish(total, passed, failed);
            out.push(t);
        }
    };

    for block in &blocks {
        if let Block::Heading(level, inl) = block {
            let text = plain(inl);
            if let Some(id) = leading_id(&text, &prefixes) {
                close(current.take(), &mut out);
                let title = text[id.len()..].trim_start_matches([' ', ':', '-', '–', '—', '.']).trim().to_string();
                let mut ev = BTreeSet::new();
                cites(inl, &mut ev);
                let t = Test {
                    id,
                    step: step.to_string(),
                    title,
                    requirements: ids(&text, "URS"),
                    outcome: Outcome::NotExecuted,
                    steps_total: 0,
                    steps_passed: 0,
                    steps_failed: 0,
                    evidence: ev.into_iter().collect(),
                };
                current = Some((*level as usize, t, Vec::new()));
                continue;
            }
            // A heading at the same or a higher level ends the test's section.
            if current.as_ref().is_some_and(|(l, _, _)| (*level as usize) <= *l) {
                close(current.take(), &mut out);
            }
        }

        // Tests written as table rows: ID first, with a result column.
        if current.is_none()
            && let Block::Table { head, rows } = block
        {
            let names: Vec<String> = head.iter().map(|h| plain(h)).collect();
            let result_col = column(&names, &["pass/fail", "result", "outcome", "status"], &["pass", "result"]);
            for row in rows {
                let first = row.first().map(|c| plain(c)).unwrap_or_default();
                let Some(id) = leading_id(&first, &prefixes) else { continue };
                let all: Vec<String> = row.iter().map(|c| plain(c)).collect();
                let result: Vec<String> = result_col.and_then(|i| all.get(i).cloned()).into_iter().collect();
                let (total, passed, failed) = outcome_of(&result);
                let mut ev = BTreeSet::new();
                row.iter().for_each(|c| cites(c, &mut ev));
                out.push(Test {
                    id,
                    step: step.to_string(),
                    title: all.get(1).cloned().unwrap_or_default().chars().take(200).collect(),
                    requirements: ids(&all.join(" "), "URS"),
                    outcome: finish(total, passed, failed),
                    steps_total: total,
                    steps_passed: passed,
                    steps_failed: failed,
                    evidence: ev.into_iter().collect(),
                });
            }
            continue;
        }

        if let Some((_, test, cells)) = current.as_mut() {
            for id in ids(&block_text(block), "URS") {
                if !test.requirements.contains(&id) {
                    test.requirements.push(id);
                }
            }
            let mut ev: BTreeSet<String> = test.evidence.iter().cloned().collect();
            block_cites(block, &mut ev);
            test.evidence = ev.into_iter().collect();
            if let Block::Table { head, rows } = block {
                let names: Vec<String> = head.iter().map(|h| plain(h)).collect();
                if let Some(col) = column(&names, &["pass/fail", "pass / fail", "result"], &["pass"]) {
                    cells.extend(rows.iter().map(|r| r.get(col).map(|c| plain(c)).unwrap_or_default()));
                }
            }
        }
    }
    close(current, &mut out);
    out
}

/// Builds the matrix from the documents' current text, keyed by step id.
pub fn build(docs: &BTreeMap<String, String>) -> Matrix {
    let mut warnings = Vec::new();
    let mut reqs = docs.get("requirements").map(|b| requirements(b)).unwrap_or_default();
    if docs.get("requirements").is_none() {
        warnings.push("No requirements document yet: draft the Requirements and risk step first.".to_string());
    } else if reqs.is_empty() {
        warnings.push("The requirements document has no table rows with IDs like URS-001.".to_string());
    }
    // Keep the first of any duplicated requirement ID.
    let mut seen = BTreeSet::new();
    reqs.retain(|r| {
        let fresh = seen.insert(r.id.clone());
        if !fresh {
            warnings.push(format!("{} is defined more than once in the requirements; the first is used.", r.id));
        }
        fresh
    });

    let mut specs = Vec::new();
    for step in ["specification", "configuration"] {
        if let Some(b) = docs.get(step) {
            specs.extend(spec_items(b, step));
        }
    }
    let mut tests_all = Vec::new();
    for step in ["installation", "testing"] {
        if let Some(b) = docs.get(step) {
            tests_all.extend(tests(b, step));
        }
    }
    let mut seen = BTreeSet::new();
    tests_all.retain(|t| {
        let fresh = seen.insert(t.id.clone());
        if !fresh {
            warnings.push(format!("{} appears more than once; the first is used.", t.id));
        }
        fresh
    });

    let known: BTreeSet<&str> = reqs.iter().map(|r| r.id.as_str()).collect();
    for t in &tests_all {
        for r in t.requirements.iter().filter(|r| !known.contains(r.as_str())) {
            warnings.push(format!("{} covers {r}, which is not in the requirements.", t.id));
        }
        if t.requirements.is_empty() && t.step == "testing" {
            warnings.push(format!("{} does not say which requirements it covers.", t.id));
        }
    }
    for s in &specs {
        for r in s.requirements.iter().filter(|r| !known.contains(r.as_str())) {
            warnings.push(format!("{} refers to {r}, which is not in the requirements.", s.id));
        }
    }

    let mut summary = Summary { requirements: reqs.len(), tests: tests_all.len(), ..Default::default() };
    let rows: Vec<Row> = reqs
        .into_iter()
        .map(|r| {
            let covering: Vec<&Test> = tests_all.iter().filter(|t| t.requirements.contains(&r.id)).collect();
            let status = if covering.is_empty() {
                Status::NotTested
            } else if covering.iter().any(|t| t.outcome == Outcome::Failed) {
                Status::Failed
            } else if covering.iter().all(|t| t.outcome == Outcome::Passed) {
                Status::Passed
            } else {
                Status::NotExecuted
            };
            match status {
                Status::Passed => summary.passed += 1,
                Status::Failed => summary.failed += 1,
                Status::NotExecuted => summary.not_executed += 1,
                Status::NotTested => summary.not_tested += 1,
            }
            let high_risk = r.risk.starts_with("high");
            if high_risk && status != Status::Passed {
                summary.high_risk_open += 1;
            }
            Row {
                specs: specs.iter().filter(|s| s.requirements.contains(&r.id)).map(|s| s.id.clone()).collect(),
                tests: covering.iter().map(|t| t.id.clone()).collect(),
                status,
                high_risk,
                requirement: r,
            }
        })
        .collect();

    Matrix { rows, tests: tests_all, specs, summary, warnings }
}

pub fn status_label(s: Status) -> &'static str {
    match s {
        Status::Passed => "Passed",
        Status::Failed => "Failed",
        Status::NotExecuted => "Not yet executed",
        Status::NotTested => "No test",
    }
}

/// The Traceability step's document, written from the matrix.
pub fn markdown(m: &Matrix, built_from: &[(String, String, i64)], built_at: &str) -> String {
    let mut d = String::from("# Traceability matrix\n\n");
    let from: Vec<String> = built_from.iter().map(|(step, title, v)| format!("{title} version {v} [D:{step}]")).collect();
    d.push_str(&format!(
        "> Built by Peter on {built_at} from {}. Every row is read from those documents; nothing here is written by AI.\n\n",
        if from.is_empty() { "no documents".to_string() } else { from.join(", ") }
    ));
    let s = &m.summary;
    d.push_str("## Summary\n\n");
    d.push_str(&format!("- Requirements: {}\n- Passed: {}\n- Failed: {}\n- Tested but not yet executed: {}\n- Without a test: {}\n- High-risk requirements not yet passed: {}\n- Test scripts: {}\n\n",
        s.requirements, s.passed, s.failed, s.not_executed, s.not_tested, s.high_risk_open, s.tests));

    d.push_str("## Matrix\n\n| ID | Requirement | Risk | Specification | Tests | Result | Evidence |\n| --- | --- | --- | --- | --- | --- | --- |\n");
    let cell = |t: &str| t.replace('|', "\\|").replace('\n', " ");
    for r in &m.rows {
        let tests: Vec<String> = r
            .tests
            .iter()
            .map(|id| {
                let t = m.tests.iter().find(|t| &t.id == id);
                match t.map(|t| t.outcome) {
                    Some(Outcome::Passed) => format!("{id} (passed)"),
                    Some(Outcome::Failed) => format!("{id} (failed)"),
                    _ => format!("{id} (not executed)"),
                }
            })
            .collect();
        let mut evidence: BTreeSet<String> = r.requirement.evidence.iter().cloned().collect();
        for id in &r.tests {
            if let Some(t) = m.tests.iter().find(|t| &t.id == id) {
                evidence.extend(t.evidence.iter().cloned());
            }
        }
        let evidence: String = evidence.iter().map(|e| format!("[{e}]")).collect();
        let result = if r.status == Status::NotTested {
            format!("[GAP: no test covers {}]", r.requirement.id)
        } else {
            status_label(r.status).to_string()
        };
        d.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            r.requirement.id,
            cell(&r.requirement.text),
            if r.requirement.risk.is_empty() { "—".into() } else { cell(&r.requirement.risk) },
            if r.specs.is_empty() { "—".to_string() } else { r.specs.join(", ") },
            if tests.is_empty() { "—".to_string() } else { tests.join(", ") },
            result,
            if evidence.is_empty() { "—".to_string() } else { evidence },
        ));
    }
    if m.rows.is_empty() {
        d.push_str("| — | — | — | — | — | [GAP: no requirements found] | — |\n");
    }

    let open: Vec<&Row> = m.rows.iter().filter(|r| r.status != Status::Passed).collect();
    d.push_str("\n## Open items\n\n");
    if open.is_empty() && m.warnings.is_empty() {
        d.push_str("None. Every requirement is covered by a passed test.\n");
    }
    for r in &open {
        let what = match r.status {
            Status::NotTested => "has no test",
            Status::Failed => "has a failed test",
            _ => "has tests not yet executed",
        };
        let risk = if r.high_risk { " (high risk)" } else { "" };
        d.push_str(&format!("- [GAP: {}{risk} {what}]\n", r.requirement.id));
    }
    for w in &m.warnings {
        d.push_str(&format!("- {}\n", w.replace('[', "(").replace(']', ")")));
    }
    d
}

#[cfg(test)]
mod tests_ {
    use super::*;

    const REQS: &str = "# User requirements\n\n| ID | Requirement | Category | GxP critical | Risk | Evidence |\n| --- | --- | --- | --- | --- | --- |\n| URS-001 | The system shall require sign-in. | Security | Yes | High | [O3] |\n| **URS-002** | The system shall log in samples. | Functional | Yes | Medium | [O2][S1] |\n| URS-003 | The system shall keep an audit trail. | Audit trail | Yes | High | [GAP: not observed] |\n| URS-004 | The system shall print labels. | Functional | No | Low | [S1] |\n";

    const TESTS: &str = "# Test scripts\n\n### TS-001 Sign-in by an authorised user\n\nRequirements covered: URS-001\n\n| Step | Action | Expected result | Actual result | Evidence | Pass/Fail |\n| --- | --- | --- | --- | --- | --- |\n| 1 | Sign in | Home shown | Home shown | [O3] | Pass |\n| 2 | Sign out | Login shown | Login shown | [O4] | Pass |\n\n### TS-002 Sample login\n\nRequirements covered: URS-002, URS-009\n\n| Step | Action | Expected | Actual | Evidence | Pass/Fail |\n| --- | --- | --- | --- | --- | --- |\n| 1 | Log in sample | Logged in | Error shown | [O2] | Fail |\n\n### TS-003 Audit trail\n\nRequirements covered: URS-003\n\n| Step | Action | Expected | Actual | Evidence | Pass/Fail |\n| --- | --- | --- | --- | --- | --- |\n| 1 | Change a record | Audit entry | Not yet executed | | |\n\n## Notes\n\nURS-004 is out of scope here.\n";

    const SPEC: &str = "| ID | Meets requirement | How |\n| --- | --- | --- |\n| FS-001 | URS-001 | Login screen |\n| FS-002 | URS-002, URS-003 | Sample screen |\n";

    #[test]
    fn ids_are_found_once_and_not_inside_words() {
        assert_eq!(ids("URS-001, URS-002 and URS-001; XURS-5 URS- URS-12", "URS"), ["URS-001", "URS-002", "URS-12"]);
    }

    #[test]
    fn requirements_come_from_the_table() {
        let r = requirements(REQS);
        assert_eq!(r.len(), 4);
        assert_eq!(r[1].id, "URS-002");
        assert_eq!(r[1].risk, "medium");
        assert_eq!(r[1].evidence, ["O2", "S1"]);
        assert_eq!(r[0].text, "The system shall require sign-in.");
    }

    #[test]
    fn tests_come_from_headings_and_their_tables() {
        let t = tests(TESTS, "testing");
        assert_eq!(t.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["TS-001", "TS-002", "TS-003"]);
        assert_eq!((t[0].outcome, t[0].steps_total, t[0].requirements.clone()), (Outcome::Passed, 2, vec!["URS-001".to_string()]));
        assert_eq!(t[1].outcome, Outcome::Failed);
        assert_eq!(t[1].requirements, ["URS-002", "URS-009"]);
        assert_eq!(t[2].outcome, Outcome::NotExecuted);
        // The "Notes" heading ends TS-003, so URS-004 is not counted as covered.
        assert_eq!(t[2].requirements, ["URS-003"]);
        assert_eq!(t[0].evidence, ["O3", "O4"]);
    }

    #[test]
    fn matrix_links_and_flags() {
        let docs: BTreeMap<String, String> = [
            ("requirements".to_string(), REQS.to_string()),
            ("testing".to_string(), TESTS.to_string()),
            ("specification".to_string(), SPEC.to_string()),
        ]
        .into();
        let m = build(&docs);
        let status: Vec<(&str, Status)> = m.rows.iter().map(|r| (r.requirement.id.as_str(), r.status)).collect();
        assert_eq!(status, [
            ("URS-001", Status::Passed),
            ("URS-002", Status::Failed),
            ("URS-003", Status::NotExecuted),
            ("URS-004", Status::NotTested),
        ]);
        assert_eq!(m.rows[1].specs, ["FS-002"]);
        assert_eq!(m.summary.high_risk_open, 1); // URS-003
        assert!(m.warnings.iter().any(|w| w.contains("TS-002 covers URS-009")));

        let md = markdown(&m, &[("requirements".into(), "User requirements".into(), 2)], "2026-11-01 08:00 UTC");
        assert!(md.contains("| URS-001 | The system shall require sign-in. | high | FS-001 | TS-001 (passed) | Passed | [O3][O4] |"), "{md}");
        assert!(md.contains("[GAP: no test covers URS-004]"));
        assert!(md.contains("- [GAP: URS-003 (high risk) has tests not yet executed]"));
    }

    #[test]
    fn tests_in_a_single_table_are_read_too() {
        let body = "| ID | Title | Requirements | Result |\n| --- | --- | --- | --- |\n| IQ-001 | Version check | — | Pass |\n| TS-010 | Report | URS-002 | Fail |\n";
        let t = tests(body, "installation");
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].outcome, t[1].outcome), (Outcome::Passed, Outcome::Failed));
        assert_eq!(t[1].requirements, ["URS-002"]);
    }
}
