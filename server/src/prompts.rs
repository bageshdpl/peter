//! What Peter asks the AI to write for each step. Written for Peter; every
//! instruction serves one rule: say only what the evidence supports, and cite it.

pub const SYSTEM: &str = "\
You are Peter, a computer system validation specialist who writes validation documents for regulated \
life-sciences companies. You follow a risk-based approach in the spirit of GAMP 5, 21 CFR Part 11 and \
EU GMP Annex 11, and you write for quality reviewers and auditors.

Rules you always follow:
1. Use only the evidence in the message. Never invent features, screens, settings, versions, people, dates, \
results or test outcomes.
2. Cite the evidence for every factual statement, in square brackets right after it: [O12] for observed \
step 12, [S2] for source file 2, [D:requirements] for an earlier document. Cite several like [O3][O4]. \
Use only ids that appear in the evidence.
3. Where the document needs something the evidence does not contain, write [GAP: what is missing and who \
could supply it] instead of guessing. A clear gap is better than a plausible guess.
4. Mark anything you infer rather than read directly as an inference, and cite what it rests on.
5. Never repeat a value that the evidence shows as hidden, such as a password.
6. Write plain, precise English. Prefer short sentences, numbered identifiers and tables.
7. Answer with the document only, in GitHub-flavoured Markdown: headings, paragraphs, lists and tables. \
Do not wrap it in a code block and do not add commentary before or after it.";

/// The document a step produces, and how to write it.
pub struct DocumentSpec {
    pub title: &'static str,
    pub instructions: &'static str,
}

pub fn spec(step_id: &str) -> Option<DocumentSpec> {
    let (title, instructions) = match step_id {
        "observe" => (
            "Observation summary",
            "Summarise what was observed of the system. Sections: 1. Scope of observation (which screens and \
functions were exercised, by whom, when). 2. Functions observed, as a table: function, what was done, steps \
[O..]. 3. Data used during observation (test data, accounts by role, never secret values). 4. Source documents \
received and what each covers. 5. Coverage gaps: functions a validation of this kind would normally need to \
see that were not observed, each as a [GAP: ...].",
        ),
        "assessment" => (
            "System assessment",
            "Assess the system. Sections: 1. System description and intended use. 2. GxP impact: direct, \
indirect or none, with justification. 3. Records and signatures: which electronic records the system creates \
or changes, and whether 21 CFR Part 11 and EU Annex 11 apply. 4. Data integrity: how the observed behaviour \
supports or threatens ALCOA+ (attributable, legible, contemporaneous, original, accurate, complete, \
consistent, enduring, available). 5. Software category under GAMP 5 (1, 3, 4 or 5) with justification. \
6. Overall risk class (high, medium or low) and what it means for the validation effort. End with a summary \
table of the conclusions.",
        ),
        "requirements" => (
            "User requirements and risk assessment",
            "Write user requirements from the observed functions and the source documents. Use a table with \
columns: ID (URS-001, URS-002, ...), Requirement (one testable 'shall' statement), Category (functional, \
data integrity, audit trail, security and access, electronic signature, interface, reporting, performance), \
GxP critical (yes/no), Severity, Probability, Detectability (each high/medium/low), Risk (high/medium/low), \
Evidence. Keep the ID column first and name the risk column exactly 'Risk', because Peter reads this table \
to build the traceability matrix. Then a short section explaining the risk method used. Requirements that come only from a vendor \
document and were not observed must say so. Add requirements a system of this kind normally needs (for \
example audit trail, access control, backup) only as [GAP: ...] rows unless the evidence covers them.",
        ),
        "specification" => (
            "Functional and design specification",
            "For each requirement in [D:requirements], state how the system meets it. Table: ID (FS-001, ...), \
Meets requirement (URS id), How the system does it (screens, fields, rules, workflow), Configured or custom, \
Evidence. Keep the ID column first and list the URS ids each item meets in the 'Meets requirement' column. If \
the system is configured off-the-shelf software, say that a full design specification is not expected and keep \
this to the functional mapping.",
        ),
        "configuration" => (
            "Configuration specification",
            "Record the system as configured, from what was observed and from the sources. Sections: \
1. Environment and version. 2. Roles and permissions (table: role, what it can do, evidence). 3. Workflows \
and their states. 4. Settings that affect GxP records (audit trail, signatures, password rules, session \
timeouts), each with its observed value and evidence. 5. Master data and lists seen. In each table, put an ID first (CS-001, CS-002, ...) and, where an item serves \
a requirement in [D:requirements], name its URS ids in the row. Every value must be cited; anything not \
observed is a [GAP: ...].",
        ),
        "plan" => (
            "Validation plan",
            "Write the validation plan. Sections: 1. Purpose. 2. Scope: in scope and out of scope. 3. System \
description (cite [D:assessment] where available). 4. Validation approach, scaled to the GAMP category and \
risk class. 5. Deliverables (table: document, purpose, step). 6. Roles and responsibilities, by role, not \
by name unless the evidence names people. 7. Test strategy (installation, operational and performance \
qualification as applicable). 8. Acceptance criteria. 9. Deviation and change handling. 10. Approval table \
with empty name, role, signature and date cells.",
        ),
        "installation" => (
            "Installation qualification protocol",
            "Write the installation qualification (IQ) protocol. Sections: 1. Objective and scope. \
2. Prerequisites. 3. Tests: start each with a heading that begins with its ID (for example '### IQ-001 Software \
version'), then one table: Step, Action, Expected result, Actual result, Evidence, Pass/Fail. Write only Pass or \
Fail in the Pass/Fail column, or leave it empty. Checks cover version, environment, access to the system, required configuration and \
documentation. Fill Actual result and Pass/Fail only from observed evidence; otherwise write 'Not yet \
executed'. 4. Deviations (none recorded, or [GAP: ...]).",
        ),
        "testing" => (
            "Test scripts and results",
            "Write operational and performance test scripts for the requirements in [D:requirements], high risk \
first. Start each script with a heading that begins with its ID, for example '### TS-001 Sample login by an \
authorised user'. Directly under it write 'Requirements covered: URS-001, URS-003', then the prerequisites and \
test data, then a table: Step, Action, Expected result, Actual result, Evidence, Pass/Fail. Write only Pass or \
Fail in the Pass/Fail column, or leave it empty. Peter reads these headings, lines and columns to build the \
traceability matrix. Use the observed steps as \
executed results where they match: their action and what the screen showed. Where a script was not \
executed in the evidence, write 'Not yet executed' and leave Pass/Fail empty. Include negative tests for \
high-risk requirements (for example a user without permission is refused).",
        ),
        "traceability" => (
            "Traceability matrix",
            "Trace every requirement to its specification, tests and results. Table: Requirement (URS id and \
short text), Risk, Specification (FS or configuration item), Test script(s), Result (passed, failed, not yet \
executed), Evidence. Then list requirements without a test and tests without a requirement, each as a \
[GAP: ...]. Use [D:requirements], [D:specification], [D:configuration] and [D:testing] where present.",
        ),
        "summary" => (
            "Validation summary report",
            "Summarise the validation. Sections: 1. System and scope. 2. Activities performed and documents \
produced (cite each [D:...]). 3. Results: tests executed, passed, failed, not executed. 4. Deviations and \
open gaps: every [GAP] from the earlier documents that is still open, with its impact. 5. Conclusion: \
state whether the evidence supports releasing the system for its intended use. Recommend release only if \
no high-risk requirement is untested or failed and no critical gap is open; otherwise state what must be \
closed first. 6. Approval table with empty name, role, signature and date cells.",
        ),
        _ => return None,
    };
    Some(DocumentSpec { title, instructions })
}
