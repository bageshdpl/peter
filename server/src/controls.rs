//! Controls: what an SOP or a regulation requires, linked to the session's
//! requirements and through them to tests. Plain rules, no AI, like the
//! traceability matrix.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::trace::{self, Matrix, Status};

/// Regulations whose clauses become controls. GAMP 5 categories classify
/// software rather than require anything of it, so they are left out.
pub fn standard_has_controls(id: &str) -> bool {
    id != "gamp5"
}

// ---------------------------------------------------------------------------
// Finding requirement statements in an SOP

/// A "shall" or "must" statement found in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// The section number it sits under ("5.2"), if the document numbers them.
    pub section: String,
    pub text: String,
}

pub const MAX_STATEMENTS: usize = 300;
const MIN_LEN: usize = 15;
const MAX_LEN: usize = 600;

/// A leading section number ("5.2.1", "5.2.1.", "4)") or bullet, and the rest.
fn split_marker(line: &str) -> (Option<String>, bool, &str) {
    let t = line.trim_start();
    for bullet in ["- ", "* ", "• ", "– ", "— ", "· ", "▪ ", "○ "] {
        if let Some(rest) = t.strip_prefix(bullet) {
            return (None, true, rest.trim_start());
        }
    }
    // "a) ", "(a) ", "i. " style list markers.
    let letter = |rest: &str| -> Option<usize> {
        let b = rest.as_bytes();
        let start = usize::from(b.first() == Some(&b'('));
        let mut i = start;
        while i < b.len() && i - start < 4 && b[i].is_ascii_lowercase() {
            i += 1;
        }
        (i > start && i < b.len() && (b[i] == b')' || (start == 0 && b[i] == b'.')) && b.get(i + 1) == Some(&b' ')).then_some(i + 2)
    };
    if let Some(end) = letter(t)
        && end - 2 <= 2
    {
        return (None, true, t[end..].trim_start());
    }
    // Numbers: digits separated by dots, then "." or ")" or a space.
    let b = t.as_bytes();
    let mut i = 0;
    let mut groups = 0;
    loop {
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start || i - start > 3 {
            return (None, false, t);
        }
        groups += 1;
        if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
            i += 1;
            continue;
        }
        break;
    }
    let number = &t[..i];
    let rest = &t[i..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')).unwrap_or(rest);
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return (None, false, t);
    }
    // "5 mg" or "10 days" is a number in a sentence, not a section.
    let rest = rest.trim_start();
    if groups == 1 && rest.chars().next().is_some_and(|c| c.is_lowercase()) {
        return (None, false, t);
    }
    (Some(number.to_string()), false, rest)
}

fn is_requirement(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    words.iter().any(|w| *w == "shall" || *w == "must")
        || words.windows(3).any(|w| (w[0] == "is" || w[0] == "are") && w[1] == "required" && w[2] == "to")
}

/// Splits a paragraph at sentence ends: ". " or "; " followed by a capital
/// letter or a digit.
fn sentences(paragraph: &str) -> Vec<String> {
    let chars: Vec<char> = paragraph.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '.' | ';' | '?' | '!') && chars.get(i + 1).is_some_and(|n| n.is_whitespace()) {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if chars.get(j).is_some_and(|n| n.is_uppercase() || n.is_ascii_digit()) {
                out.push(chars[start..=i].iter().collect::<String>());
                start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if start < chars.len() {
        out.push(chars[start..].iter().collect());
    }
    out
}

/// The requirement statements in a document's text, in order, without repeats.
pub fn statements(text: &str) -> Vec<Statement> {
    // Lines are joined into paragraphs: PDFs break sentences across lines.
    let mut paragraphs: Vec<(String, String)> = Vec::new(); // (section, text)
    let mut section = String::new();
    let mut current = String::new();
    let mut current_section = String::new();
    let flush = |current: &mut String, current_section: &str, out: &mut Vec<(String, String)>| {
        let t = current.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t.is_empty() {
            out.push((current_section.to_string(), t));
        }
        current.clear();
    };
    for raw in text.lines() {
        // Table cells (tabs) and pipes become separate pieces of text.
        for line in raw.split(['\t', '|']) {
            let line = line.trim();
            if line.is_empty() {
                if raw.trim().is_empty() {
                    flush(&mut current, &current_section, &mut paragraphs);
                }
                continue;
            }
            let (number, bullet, rest) = split_marker(line.trim_start_matches('#').trim_start());
            if number.is_some() || bullet || raw.contains(['\t', '|']) || line.starts_with('#') {
                flush(&mut current, &current_section, &mut paragraphs);
                if let Some(n) = number {
                    section = n;
                }
                current_section = section.clone();
            } else if current.is_empty() {
                current_section = section.clone();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(rest);
        }
    }
    flush(&mut current, &current_section, &mut paragraphs);

    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (section, paragraph) in paragraphs {
        for s in sentences(&paragraph) {
            let s = s.trim().trim_end_matches([';', ',']).trim().to_string();
            let len = s.chars().count();
            if !(MIN_LEN..=MAX_LEN).contains(&len) || !is_requirement(&s) {
                continue;
            }
            if seen.insert(normalize(&s)) {
                out.push(Statement { section: section.clone(), text: s });
                if out.len() == MAX_STATEMENTS {
                    return out;
                }
            }
        }
    }
    out
}

/// Text compared for repeats: lower case, letters and digits only.
pub fn normalize(text: &str) -> String {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Finding which controls a requirement names

/// Part 11 clauses a text names, such as "21 CFR 11.10(e)" or "§11.50".
/// Only read when the text also says "CFR" or "Part 11", so version numbers
/// like "11.50" are not mistaken for clauses.
pub fn part11_refs(text: &str, clauses: &[&str]) -> BTreeSet<String> {
    let lower = text.to_lowercase().replace(" (", "(");
    let mut found = BTreeSet::new();
    if !(lower.contains("cfr") || lower.contains("part 11")) {
        return found;
    }
    for clause in clauses {
        let pat = clause.to_lowercase();
        let mut from = 0;
        while let Some(pos) = lower[from..].find(&pat).map(|p| p + from) {
            let before_ok = lower[..pos].chars().last().is_none_or(|c| !c.is_ascii_digit() && c != '.');
            let after_ok = lower[pos + pat.len()..].chars().next().is_none_or(|c| !c.is_ascii_digit() && (pat.ends_with(')') || c != '('));
            if before_ok && after_ok {
                found.insert(clause.to_string());
                break;
            }
            from = pos + pat.len();
        }
    }
    found
}

/// Annex 11 clauses a text names: "Annex 11, clause 9", "Annex 11 §7.1",
/// "Annex 11 clauses 7 and 9", "Annex 11 principle".
pub fn annex11_refs(text: &str) -> BTreeSet<String> {
    let lower = text.to_lowercase();
    let mut found = BTreeSet::new();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("annex 11").map(|p| p + from) {
        let mut rest = &lower[pos + "annex 11".len()..];
        from = pos + "annex 11".len();
        if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue; // "Annex 110"
        }
        let skip = |r: &str| -> usize {
            let mut r2 = r;
            loop {
                let before = r2.len();
                r2 = r2.trim_start_matches([' ', ',', ':', '§', '(', '-', '–']);
                for word in ["clauses", "clause", "sections", "section", "paragraphs", "paragraph", "para.", "para", "cl.", "and", "&", "no.", "nr."] {
                    if let Some(x) = r2.strip_prefix(word)
                        && !x.chars().next().is_some_and(|c| c.is_alphabetic())
                    {
                        r2 = x;
                    }
                }
                if r2.len() == before {
                    return r.len() - r2.len();
                }
            }
        };
        rest = &rest[skip(rest)..];
        if rest.starts_with("principle") {
            found.insert("Principle".to_string());
            continue;
        }
        // One or more numbers: "7", "7.1" (counted as clause 7), "7, 9 and 12".
        loop {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                break;
            }
            let n: u32 = digits.parse().unwrap_or(0);
            if (1..=17).contains(&n) {
                found.insert(n.to_string());
            }
            rest = &rest[digits.len()..];
            // Skip a sub-clause such as ".1".
            if let Some(r) = rest.strip_prefix('.')
                && r.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                rest = r.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
            }
            let n = skip(rest);
            let next = &rest[n..];
            // Only continue a list ("7, 9 and 12"), not into unrelated numbers.
            let joined = rest[..n].contains([',', '&']) || rest[..n].contains("and");
            if n == 0 || !joined || !next.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                break;
            }
            rest = next;
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Coverage

/// A control as the session sees it.
#[derive(Debug, Clone)]
pub struct Control {
    pub key: String,
    /// "21 CFR Part 11", or "Company SOPs".
    pub group: String,
    /// "11.10(e)", "9", or "CTL-003".
    pub reference: String,
    pub title: String,
    pub text: String,
    /// The SOP file the control came from.
    pub source: Option<String>,
    pub detect: Detect,
}

#[derive(Debug, Clone)]
pub enum Detect {
    Part11(String),
    Annex11(String),
    /// A project control, named in requirements by its ID ("CTL-003").
    Id(String),
    /// Linked by hand only.
    Manual,
}

/// A session's decisions about a control.
#[derive(Debug, Clone, Default)]
pub struct Decision {
    pub requirements: Vec<String>,
    pub not_applicable: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlStatus {
    /// Every linked requirement passed its tests.
    Met,
    Failed,
    NotExecuted,
    /// A requirement covers it but has no test.
    Untested,
    /// No requirement covers it.
    NotCovered,
    NotApplicable,
}

pub fn status_label(s: ControlStatus) -> &'static str {
    match s {
        ControlStatus::Met => "Met",
        ControlStatus::Failed => "Failed",
        ControlStatus::NotExecuted => "Not yet executed",
        ControlStatus::Untested => "No test",
        ControlStatus::NotCovered => "Not covered",
        ControlStatus::NotApplicable => "Not applicable",
    }
}

#[derive(Debug, Serialize)]
pub struct ControlRow {
    pub key: String,
    pub group: String,
    pub reference: String,
    pub title: String,
    pub text: String,
    pub source: Option<String>,
    /// Requirements that name the control themselves.
    pub found: Vec<String>,
    /// Requirements linked by hand.
    pub linked: Vec<String>,
    pub tests: Vec<String>,
    pub not_applicable: bool,
    pub reason: String,
    pub status: ControlStatus,
}

#[derive(Debug, Default, Serialize)]
pub struct CoverageSummary {
    pub controls: usize,
    pub met: usize,
    pub failed: usize,
    pub not_executed: usize,
    pub untested: usize,
    pub not_covered: usize,
    pub not_applicable: usize,
}

#[derive(Debug, Serialize)]
pub struct Coverage {
    pub rows: Vec<ControlRow>,
    pub summary: CoverageSummary,
    pub warnings: Vec<String>,
}

pub fn coverage(controls: &[Control], decisions: &BTreeMap<String, Decision>, matrix: &Matrix) -> Coverage {
    let mut summary = CoverageSummary { controls: controls.len(), ..Default::default() };
    let mut warnings = Vec::new();
    let known: BTreeSet<&str> = matrix.rows.iter().map(|r| r.requirement.id.as_str()).collect();
    let mut rows = Vec::new();
    for c in controls {
        let found: Vec<String> = matrix
            .rows
            .iter()
            .filter(|r| {
                let t = &r.requirement.row_text;
                match &c.detect {
                    Detect::Part11(clause) => part11_refs(t, &[clause.as_str()]).contains(clause),
                    Detect::Annex11(clause) => annex11_refs(t).contains(clause),
                    Detect::Id(id) => trace::ids(t, "CTL").contains(id),
                    Detect::Manual => false,
                }
            })
            .map(|r| r.requirement.id.clone())
            .collect();
        let decision = decisions.get(&c.key).cloned().unwrap_or_default();
        for id in decision.requirements.iter().filter(|id| !known.contains(id.as_str())) {
            warnings.push(format!("{} {} is linked to {id}, which is not in the requirements.", c.group, c.reference));
        }
        let linked: Vec<String> = decision.requirements.iter().filter(|id| !found.contains(id)).cloned().collect();
        let all: BTreeSet<&String> = found.iter().chain(linked.iter()).filter(|id| known.contains(id.as_str())).collect();
        let statuses: Vec<Status> = matrix.rows.iter().filter(|r| all.contains(&r.requirement.id)).map(|r| r.status).collect();
        let mut tests: Vec<String> = Vec::new();
        for r in matrix.rows.iter().filter(|r| all.contains(&r.requirement.id)) {
            for t in &r.tests {
                if !tests.contains(t) {
                    tests.push(t.clone());
                }
            }
        }
        let status = if decision.not_applicable {
            ControlStatus::NotApplicable
        } else if statuses.is_empty() {
            ControlStatus::NotCovered
        } else if statuses.contains(&Status::Failed) {
            ControlStatus::Failed
        } else if statuses.contains(&Status::NotTested) {
            ControlStatus::Untested
        } else if statuses.contains(&Status::NotExecuted) {
            ControlStatus::NotExecuted
        } else {
            ControlStatus::Met
        };
        match status {
            ControlStatus::Met => summary.met += 1,
            ControlStatus::Failed => summary.failed += 1,
            ControlStatus::NotExecuted => summary.not_executed += 1,
            ControlStatus::Untested => summary.untested += 1,
            ControlStatus::NotCovered => summary.not_covered += 1,
            ControlStatus::NotApplicable => summary.not_applicable += 1,
        }
        rows.push(ControlRow {
            key: c.key.clone(),
            group: c.group.clone(),
            reference: c.reference.clone(),
            title: c.title.clone(),
            text: c.text.clone(),
            source: c.source.clone(),
            found,
            linked,
            tests,
            not_applicable: decision.not_applicable,
            reason: decision.reason,
            status,
        });
    }
    Coverage { rows, summary, warnings }
}

/// The controls of the regulations in scope, in catalog order.
pub fn from_standards(ids: &[String]) -> Vec<Control> {
    let mut out = Vec::new();
    for s in crate::standards::CATALOG.iter().filter(|s| ids.iter().any(|id| id == s.id) && standard_has_controls(s.id)) {
        for c in s.clauses {
            let detect = match s.id {
                "part11" => Detect::Part11(c.reference.to_string()),
                "annex11" => Detect::Annex11(c.reference.to_string()),
                _ => Detect::Manual,
            };
            out.push(Control {
                key: standard_key(s.id, c.title),
                group: s.name.to_string(),
                reference: c.reference.to_string(),
                title: c.title.to_string(),
                text: c.summary.to_string(),
                source: None,
                detect,
            });
        }
    }
    out
}

/// A regulation clause's key. Titles are unique within each regulation,
/// while some references are not (ALCOA+ has two "A"s).
pub fn standard_key(standard: &str, title: &str) -> String {
    format!("{standard}:{title}")
}

pub fn project_key(reference: &str) -> String {
    format!("project:{reference}")
}

pub fn reference(number: i64) -> String {
    format!("CTL-{number:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_are_found_across_line_breaks_with_their_sections() {
        let sop = "\
5 Responsibilities
5.1 The system owner shall approve every change before it is released. Users
must complete training before they get access.
Some background text that is not a requirement.

5.2 Records
a) Audit trails are required to be reviewed monthly by QA.
- Passwords must not be shared
6. Notes: This is a description; it explains things.
| SOP-7 | The vendor shall provide release notes for every version. |
";
        let s = statements(sop);
        let got: Vec<(&str, &str)> = s.iter().map(|s| (s.section.as_str(), s.text.as_str())).collect();
        assert_eq!(got, [
            ("5.1", "The system owner shall approve every change before it is released."),
            ("5.1", "Users must complete training before they get access."),
            ("5.2", "Audit trails are required to be reviewed monthly by QA."),
            ("5.2", "Passwords must not be shared"),
            ("6", "The vendor shall provide release notes for every version."),
        ]);
    }

    #[test]
    fn repeats_and_short_fragments_are_dropped() {
        let s = statements("Data shall be backed up daily.\n\nData  shall be backed up DAILY\n\nIt must.\n\nthe user SHALL sign records electronically");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].text, "the user SHALL sign records electronically");
        assert!(statements("Shallow waters are marshalled here.").is_empty());
    }

    #[test]
    fn numbers_in_sentences_are_not_sections() {
        let s = statements("10 days after release, the owner shall review the log.");
        assert_eq!(s[0].section, "");
        assert_eq!(s[0].text, "10 days after release, the owner shall review the log.");
    }

    #[test]
    fn part11_clauses_are_read_only_with_their_boundaries() {
        let clauses = ["11.10(a)", "11.10(e)", "11.30", "11.300", "11.50"];
        assert_eq!(part11_refs("Keep an audit trail (21 CFR 11.10(e)).", &clauses).into_iter().collect::<Vec<_>>(), ["11.10(e)"]);
        assert_eq!(part11_refs("Per 21 CFR §11.300 and 11.10 (A)", &clauses).into_iter().collect::<Vec<_>>(), ["11.10(a)", "11.300"]);
        // "11.50" as a version number, with no mention of the regulation.
        assert!(part11_refs("Runs on version 11.50", &clauses).is_empty());
    }

    #[test]
    fn annex11_clauses_in_their_usual_spellings() {
        let v = |t: &str| annex11_refs(t).into_iter().collect::<Vec<_>>();
        assert_eq!(v("Audit trail (Annex 11, clause 9)."), ["9"]);
        assert_eq!(v("See EU GMP Annex 11 §7.1 and data rules"), ["7"]);
        assert_eq!(v("Annex 11 clauses 4, 12 and 17"), ["12", "17", "4"]);
        assert_eq!(v("annex 11 principle applies"), ["Principle"]);
        assert!(v("Annex 11 applies to 30 systems").is_empty());
        assert!(v("Annex 110").is_empty());
    }

    fn matrix() -> Matrix {
        let reqs = "| ID | Requirement | Risk |\n| --- | --- | --- |\n\
| URS-001 | Keep an audit trail (21 CFR 11.10(e); Annex 11, clause 9). | High |\n\
| URS-002 | Approve changes before release (CTL-001). | Medium |\n\
| URS-003 | Back up data daily. | Low |\n";
        let tests = "### TS-001 Audit trail\n\nRequirements covered: URS-001\n\n| Step | Pass/Fail |\n| --- | --- |\n| 1 | Pass |\n\n\
### TS-002 Change\n\nRequirements covered: URS-002\n\n| Step | Pass/Fail |\n| --- | --- |\n| 1 | Fail |\n";
        trace::build(&[("requirements".to_string(), reqs.to_string()), ("testing".to_string(), tests.to_string())].into())
    }

    #[test]
    fn coverage_follows_requirements_to_their_tests() {
        let mut controls = from_standards(&["part11".into(), "annex11".into(), "gamp5".into()]);
        assert!(controls.iter().all(|c| !c.group.starts_with("GAMP")));
        controls.push(Control {
            key: project_key("CTL-001"),
            group: "Company SOPs".into(),
            reference: "CTL-001".into(),
            title: String::new(),
            text: "Changes shall be approved.".into(),
            source: Some("SOP-12.pdf".into()),
            detect: Detect::Id("CTL-001".into()),
        });
        let mut decisions = BTreeMap::new();
        decisions.insert(standard_key("annex11", "Data storage"), Decision { requirements: vec!["URS-003".into(), "URS-404".into()], ..Default::default() });
        decisions.insert(standard_key("part11", "Open systems"), Decision { not_applicable: true, reason: "Closed system.".into(), ..Default::default() });
        let cov = coverage(&controls, &decisions, &matrix());
        let row = |key: &str| cov.rows.iter().find(|r| r.key == key).unwrap();

        let audit = row(&standard_key("part11", "Audit trail"));
        assert_eq!((audit.status, audit.found.clone(), audit.tests.clone()), (ControlStatus::Met, vec!["URS-001".to_string()], vec!["TS-001".to_string()]));
        assert_eq!(row(&standard_key("annex11", "Audit trails")).status, ControlStatus::Met);
        assert_eq!(row(&project_key("CTL-001")).status, ControlStatus::Failed);
        let storage = row(&standard_key("annex11", "Data storage"));
        assert_eq!((storage.status, storage.linked.clone()), (ControlStatus::Untested, vec!["URS-003".to_string(), "URS-404".to_string()]));
        assert_eq!(row(&standard_key("part11", "Open systems")).status, ControlStatus::NotApplicable);
        assert_eq!(row(&standard_key("part11", "Training")).status, ControlStatus::NotCovered);
        assert!(cov.warnings.iter().any(|w| w.contains("URS-404")));
        assert_eq!(cov.summary.met, 2);
        assert_eq!(cov.summary.controls, controls.len());
    }

    #[test]
    fn regulation_keys_are_unique() {
        let all: Vec<String> = crate::standards::CATALOG.iter().map(|s| s.id.to_string()).collect();
        let controls = from_standards(&all);
        let keys: BTreeSet<&str> = controls.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys.len(), controls.len());
    }
}
