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
step 12, [S2] for source file 2, [D:requirements] for an earlier document in this session, [B:requirements] \
for a document from the earlier validation the session builds on. Cite several like [O3][O4]. Use only ids that \
appear in the evidence.
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
to build the traceability matrix. Inside a requirement's row, name the regulation clauses and company controls \
it meets, for example '(21 CFR 11.10(e); Annex 11, clause 9; CTL-003)', so Peter can show which controls are \
covered. Then a short section explaining the risk method used. Requirements that come only from a vendor \
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
        // Change validation.
        "change_description" => (
            "Change description",
            "Describe the change. Sections: 1. Change summary: what is changing, why, and the change record number \
if the evidence gives one. 2. Kind of change: configuration, new function, upgrade or patch, infrastructure or \
hosting move, or data migration, with justification. 3. Before and after: a table of item, before (cite the \
earlier validation [B:...] or observed steps), after (cite observed steps or sources), and evidence. 4. Systems, \
interfaces, records and users touched. 5. Planned timing and who requested and approved it, only as the evidence \
states; otherwise [GAP: ...].",
        ),
        "baseline" => (
            "Baseline documents",
            "List the validated baseline this change is measured against. Sections: 1. The earlier validation: which \
session, its validation type and its documents, as a table: document, what it establishes, cite [B:...]. If the \
evidence has no earlier validation, write a gap asking for the earlier validation of this system to be linked to the \
session, or its approved documents added as source files, and use any validated documents found in the source files \
instead. \
2. Baseline requirements relevant to the change (URS ids and short text, cited). 3. Baseline tests relevant to the \
change (test ids, cited). 4. Open gaps or deviations carried over from the earlier validation.",
        ),
        "impact" => (
            "Impact assessment",
            "Assess the impact of the change in [D:change_description] on the validated baseline in [D:baseline]. \
Sections: 1. Impact table with columns: ID (IA-001, ...), Area (requirement, function, configuration item, interface, \
record, procedure or training), Affected baseline item (URS, FS, CS or test id where known), Impact (none, low, medium, \
high), Why, Evidence. 2. GxP and data integrity impact, including on electronic records, signatures and audit trail. \
3. Regulatory impact: whether the change needs notification or affects the system's GAMP category. 4. Conclusion: \
change category (minor, major or critical) and the validation effort it calls for. Never mark an area as not \
affected without a reason.",
        ),
        "test_scope" => (
            "Test scope and results",
            "Decide which tests the change needs and record their results. Sections: 1. Test scope table: baseline \
test id or 'new', why it is in scope (cite [D:impact]), regression or new. 2. Tests not repeated, each with its \
justification. 3. Test scripts: start each with a heading that begins with its ID, for example '### TS-101 Report \
shows the new field', then 'Requirements covered: URS-001, URS-004' (the baseline URS ids, or new ids from URS-101 \
upwards for new requirements), then one table: Step, Action, Expected result, Actual result, Evidence, Pass/Fail. \
Write only Pass or Fail in the Pass/Fail column, or leave it empty. Use observed steps as executed results where \
they match; otherwise write 'Not yet executed'. Include a regression test for every high-impact area.",
        ),
        "document_updates" => (
            "Document updates",
            "List the changes the validated documents need because of this change, without rewriting whole documents. \
Table: Document (cite the baseline [B:...] where it exists), Section or ID, Current text (short), New text, Reason \
(cite [D:impact] or [D:test_scope]). Then new requirements in the same table format as the baseline requirements \
(ID, Requirement, Category, GxP critical, Risk, Evidence), numbered from URS-101. Then procedures and training \
material that must change. Documents that need no change are listed once with 'no change' and the reason.",
        ),
        "change_summary" => (
            "Change summary report",
            "Summarise the change validation. Sections: 1. Change and its category (cite [D:change_description] and \
[D:impact]). 2. Tests executed, passed, failed and not executed (cite [D:test_scope]). 3. Document updates made \
(cite [D:document_updates]). 4. Deviations and open gaps, with their impact. 5. Conclusion: whether the evidence \
supports releasing the change and confirms the system remains in a validated state. Recommend release only if no \
high-impact area is untested or failed. 6. Approval table with empty name, role, signature and date cells.",
        ),
        // Periodic review.
        "history" => (
            "Changes and incidents since the last review",
            "Record what happened to the system since its last validation or review. Sections: 1. Review period and \
the baseline it starts from (cite [B:...] where available). 2. Changes made: table of change, date, change record, \
whether it was validated, evidence. 3. Incidents, deviations, CAPAs and complaints related to the system. 4. Vendor \
updates, support status and end-of-life notices. 5. Changes in use, users, regulations or procedures. Where the \
evidence does not cover an area, write a [GAP: ...] naming who would hold that record.",
        ),
        "controls" => (
            "Controls check",
            "Check that the controls keeping the system validated still work. Use a table with columns: ID (PR-001, ...), \
Control (user access and periodic access review, password and session settings, audit trail enabled and reviewed, \
electronic signatures, backup and restore test, disaster recovery, configuration matches the baseline, procedures \
current, training current, vendor support, data archiving), What was checked, Result (effective, not effective, \
not checked), Evidence. Compare observed settings with the baseline configuration [B:configuration] where it exists. \
Then list every not-effective or not-checked control with what must be done.",
        ),
        "review_report" => (
            "Periodic review report",
            "Write the periodic review report. Sections: 1. System and review period. 2. Summary of changes and \
incidents (cite [D:history]). 3. Controls check results (cite [D:controls]). 4. Findings, as a table: finding, \
severity (critical, major, minor), action, owner role, due. 5. Conclusion: whether the system remains in a validated \
state and fit for its intended use, or what must happen first. 6. Recommended date of the next review, with its \
reason (risk class and findings). 7. Approval table with empty name, role, signature and date cells.",
        ),
        // Data migration.
        "migration_plan" => (
            "Data migration plan",
            "Write the data migration plan. Sections: 1. Purpose and scope: source and target systems, data sets, \
record counts and date ranges where the evidence gives them. 2. GxP criticality of each data set. 3. Migration \
method (tool, manual, vendor service) and whether the tool itself needs validation. 4. Cleansing and \
transformation rules. 5. Roles and responsibilities, by role. 6. Acceptance criteria, measurable, for example \
record counts match and 100% of critical fields verified. 7. Rollback plan. 8. Risks to data integrity (ALCOA+) and \
how each is controlled.",
        ),
        "mapping" => (
            "Data mapping specification",
            "Map each source field to its target. Table with columns: ID (MAP-001, ...), Source table and field, Target \
table and field, Data type and format, Transformation rule, Critical (yes/no), Evidence. Then sections on: values \
that change meaning (code lists, units, time zones), fields with no target and why, target fields with no source and \
their default, and how audit trail and metadata (who, when) are carried across. Every mapping must be cited; anything \
not in the evidence is a [GAP: ...].",
        ),
        "verification_plan" => (
            "Migration verification plan",
            "Plan how the migration will be verified. Sections: 1. Approach: record counts, checksums, field-by-field \
comparison of critical fields, and sampling of the rest (state the sample size and its basis). 2. Verification \
checks: start each with a heading that begins with its ID, for example '### MV-001 Record counts match', then \
'Mappings covered: MAP-001, MAP-002', then a table: Step, Action, Expected result. 3. Handling of discrepancies: \
how they are logged, assessed and fixed. 4. Acceptance criteria (cite [D:migration_plan]).",
        ),
        "migration_report" => (
            "Data migration report",
            "Report the migration and its verification. Sections: 1. What was migrated, when and by which method. \
2. Verification results: for each check in [D:verification_plan], a heading that begins with its ID, then a table: \
Step, Action, Expected result, Actual result, Evidence, Pass/Fail. Write only Pass or Fail in the Pass/Fail column, \
or leave it empty; use observed steps as executed results where they match, otherwise 'Not yet executed'. \
3. Discrepancies and their resolution. 4. Conclusion against the acceptance criteria: whether the migrated data can \
be relied on. 5. Approval table with empty name, role, signature and date cells.",
        ),
        // Retirement.
        "retirement_plan" => (
            "Retirement plan",
            "Plan the system's retirement. Sections: 1. System and reason for retirement. 2. Replacement system, if \
any, and how its data and functions are taken over. 3. GxP records the system holds, as a table: record type, \
regulation and retention period, volume, evidence (cite the earlier validation [B:...] where it lists them). \
4. Options for the records: migrate, archive in a readable format, keep the system read-only, or destroy after \
retention, with the choice for each record type and why. 5. Timeline and responsibilities, by role. 6. Risks and \
how each is controlled.",
        ),
        "archive_plan" => (
            "Data archiving plan",
            "Plan how records are kept readable after retirement. Sections: 1. Records to archive (cite \
[D:retirement_plan]). 2. Archive location and format, and why the format keeps the record complete and readable, \
including its metadata and audit trail. 3. Retention period and destruction rules. 4. Access: who can read the \
archive, how, and how long a retrieval takes. 5. Verification that the archive is complete and accurate: checks \
with IDs (AR-001, ...), each with expected result. 6. Periodic checks that the archive stays readable.",
        ),
        "decommission" => (
            "Decommissioning record",
            "Record the switch-off. Use checks with IDs: start each with a heading that begins with its ID, for example \
'### DC-001 User access removed', then a table: Step, Action, Expected result, Actual result, Evidence, Pass/Fail. \
Write only Pass or Fail in the Pass/Fail column, or leave it empty. Cover: final backup taken, archive verified, \
interfaces stopped, user accounts disabled, licences ended, servers or tenants removed or wiped, documentation \
updated, and the system inventory updated. Use observed steps as executed results where they match; otherwise write \
'Not yet executed'.",
        ),
        "retirement_report" => (
            "Retirement report",
            "Summarise the retirement. Sections: 1. System and reason. 2. What happened to each type of record (cite \
[D:retirement_plan] and [D:archive_plan]). 3. Decommissioning results (cite [D:decommission]). 4. Deviations and \
open gaps. 5. Conclusion: whether the system is fully retired and its records remain available for their retention \
period. 6. Approval table with empty name, role, signature and date cells.",
        ),
        // AI validation.
        "ai_use" => (
            "AI intended use",
            "Describe the model's intended use. Sections: 1. The decision or task the model supports, and its output \
(classification, score, text, extraction). 2. Who uses the output and how: decides alone, advises a person, or is \
checked by a person every time (human in the loop). 3. The GxP process and records it affects. 4. Inputs the model \
receives, and the population or data it is meant for. 5. Uses that are out of scope. 6. The model's provider, \
version and whether it learns after release (locked or adaptive).",
        ),
        "ai_risk" => (
            "AI context and risk assessment",
            "Assess the risk of the model being wrong. Sections: 1. Model influence (how much the decision rests on the \
model) and decision consequence (harm to patient, product or data if wrong), each high, medium or low with \
justification, and the resulting risk. 2. Failure modes table: ID (AIR-001, ...), Failure (wrong answer, bias, drift, \
made-up content, unavailable), Effect, Severity, Likelihood, Detection, Risk, Control. 3. Data integrity and privacy \
risks. 4. What the risk level means for the amount of testing and monitoring. Cite [D:ai_use].",
        ),
        "ai_data" => (
            "AI data and model description",
            "Describe the data and the model. Sections: 1. Model: type, provider, version, and how it was trained or \
configured (prompt, fine-tuning, settings), as the evidence states. 2. Training and reference data: sources, size, \
period, how it was labelled, and how representative it is of the intended use. 3. Test data: how it is kept separate \
from training data. 4. Known limits, bias and conditions where it performs poorly. 5. Version control of the model, \
its prompts and settings. Anything the vendor has not disclosed is a [GAP: ...].",
        ),
        "ai_acceptance" => (
            "AI requirements and acceptance criteria",
            "Set measurable targets. Table with columns: ID (URS-001, ...), Requirement (one testable 'shall' statement), \
Metric (accuracy, sensitivity, specificity, precision, error rate, consistency on repeat, response time), Target, \
Test data, Risk (high/medium/low), Evidence. Keep the ID column first and name the risk column exactly 'Risk'. Base \
targets on [D:ai_risk]: higher risk needs stricter targets. Include requirements for human review, explainability \
of outputs, audit trail of inputs and outputs, and behaviour on inputs outside the intended use.",
        ),
        "ai_testing" => (
            "AI performance test report",
            "Test the model against [D:ai_acceptance]. Start each test with a heading that begins with its ID, for \
example '### TS-001 Accuracy on the reference set', then 'Requirements covered: URS-001', then the test data and \
method, then a table: Step, Action, Expected result, Actual result, Evidence, Pass/Fail. Write only Pass or Fail in \
the Pass/Fail column, or leave it empty. Use observed steps as executed results where they match: the input given \
and the output shown. Include tests of inputs outside the intended use, repeated inputs (consistency), and an \
attempt to make the model produce content not supported by its input. Where a test was not executed, write 'Not yet \
executed'. End with a results table: metric, target, result, met or not met.",
        ),
        "ai_monitoring" => (
            "AI monitoring plan",
            "Plan how the model is watched in use. Sections: 1. Metrics monitored, with thresholds that trigger action \
(cite [D:ai_acceptance] and [D:ai_testing]). 2. How and how often they are measured, and by whom (role). 3. Drift \
detection in inputs and outputs. 4. User feedback and error reporting. 5. What happens when a threshold is crossed: \
investigate, retrain, roll back or stop use. 6. Change control for new model versions, prompts and settings. \
7. Periodic review interval.",
        ),
        // Equipment.
        "equipment_assessment" => (
            "Equipment assessment",
            "Assess the equipment. Sections: 1. Description: make, model, serial number, location, software and \
firmware version, as the evidence gives them. 2. Intended use and the GxP data or product it touches. 3. Group under \
USP <1058>: A (standard equipment, no measurement needing calibration), B (instrument with a measurement needing \
calibration, firmware only) or C (computerised system with configurable software), with justification. \
4. Calibration and maintenance needs. 5. Software and data: whether it creates electronic records, and whether \
21 CFR Part 11 and EU Annex 11 apply. 6. Qualification needed: for group A, state that the remaining steps can be \
skipped with this assessment as the record; for B, IQ and OQ; for C, IQ, OQ, PQ and computer system validation of \
its software.",
        ),
        "equipment_requirements" => (
            "Equipment user requirements",
            "Write the equipment's user requirements. Table with columns: ID (URS-001, ...), Requirement (one testable \
'shall' statement), Category (operating range, accuracy and precision, capacity, safety, environment, utilities, \
software and data integrity, audit trail, security and access, calibration, cleaning), GxP critical (yes/no), \
Risk (high/medium/low), Evidence. Keep the ID column first and name the risk column exactly 'Risk'. Take operating \
ranges and specifications from the vendor documents and the intended use in [D:equipment_assessment].",
        ),
        "equipment_iq" => (
            "Equipment installation qualification",
            "Write the installation qualification. Start each check with a heading that begins with its ID, for example \
'### IQ-001 Delivery matches the order', then 'Requirements covered: URS-001' where it applies, then a table: Step, \
Action, Expected result, Actual result, Evidence, Pass/Fail. Write only Pass or Fail in the Pass/Fail column, or \
leave it empty. Cover delivery and parts, documentation received, location and environment, utilities, connections \
and network, software and firmware versions, safety checks, and calibration status. Fill results only from observed \
evidence; otherwise write 'Not yet executed'.",
        ),
        "equipment_oq" => (
            "Equipment operational qualification",
            "Write the operational qualification. Start each test with a heading that begins with its ID, for example \
'### OQ-001 Temperature across the operating range', then 'Requirements covered: URS-002, URS-003', then a table: \
Step, Action, Expected result, Actual result, Evidence, Pass/Fail. Write only Pass or Fail in the Pass/Fail column, \
or leave it empty. Test each function across its operating ranges, alarms and interlocks, and for group C the \
software: access control, audit trail, data storage, backup and electronic signatures. Use observed steps as executed \
results where they match; otherwise write 'Not yet executed'.",
        ),
        "equipment_pq" => (
            "Equipment performance qualification",
            "Write the performance qualification. Start each test with a heading that begins with its ID, for example \
'### PQ-001 Repeatability with the reference standard', then 'Requirements covered: URS-...', then a table: Step, \
Action, Expected result, Actual result, Evidence, Pass/Fail. Write only Pass or Fail in the Pass/Fail column, or \
leave it empty. Test the equipment under real conditions of use, with real or representative samples, over enough \
runs to show it performs consistently. Use observed steps as executed results where they match; otherwise write \
'Not yet executed'.",
        ),
        "equipment_report" => (
            "Equipment qualification report",
            "Summarise the qualification. Sections: 1. Equipment and group (cite [D:equipment_assessment]). \
2. Qualification performed: IQ, OQ and PQ, each with tests executed, passed, failed and not executed (cite \
[D:equipment_iq], [D:equipment_oq], [D:equipment_pq] where present; a step not done must be justified by the group). \
3. Requirements traceability: each URS id from [D:equipment_requirements] with the tests that cover it and their \
result. 4. Deviations and open gaps. 5. Calibration and maintenance arrangements before use. 6. Conclusion: whether \
the equipment is released for its intended use. 7. Approval table with empty name, role, signature and date cells.",
        ),
        // Dashboards and reports.
        "report_use" => (
            "Report use assessment",
            "Assess the report or dashboard. Sections: 1. What it shows and where it runs. 2. Who uses it and which \
GxP decisions are made from it (batch release, trending, deviations, management review). 3. Risk if a figure is \
wrong, missing or out of date, as high, medium or low with justification. 4. Data sources and how data reaches it \
(direct query, extract, manual upload). 5. Whether it is configured from standard functions or custom-built \
(queries, calculations, scripts). 6. Verification effort the risk calls for.",
        ),
        "report_requirements" => (
            "Report specification",
            "Specify every figure the report shows. Table with columns: ID (URS-001, ...), Requirement (one testable \
'shall' statement, for example 'shall show the count of open deviations by site'), Source (system, table, field), \
Calculation or aggregation, Filters and parameters, Refresh frequency, Risk (high/medium/low), Evidence. Keep the ID \
column first and name the risk column exactly 'Risk'. Then requirements for access control, data freshness \
indication, export, and behaviour when data is missing.",
        ),
        "report_verification" => (
            "Report verification",
            "Verify the report against [D:report_requirements]. Start each check with a heading that begins with its ID, \
for example '### TS-001 Open deviations by site match the source', then 'Requirements covered: URS-001', then a \
table: Step, Action, Expected result, Actual result, Evidence, Pass/Fail. Write only Pass or Fail in the Pass/Fail \
column, or leave it empty. Compare each figure with its source data, including edge cases: empty periods, filters \
combined, very large values, time zones, and records changed after the last refresh. Check that people without \
access cannot see the report. Use observed steps as executed results where they match; otherwise write 'Not yet \
executed'.",
        ),
        "report_summary" => (
            "Report verification summary",
            "Summarise the verification. Sections: 1. Report and its risk (cite [D:report_use]). 2. Figures verified: \
each URS id with its checks and result (cite [D:report_verification]). 3. Deviations and open gaps. 4. Conclusion: \
whether the report can be relied on for its decisions. 5. Change control: what change to the report, its queries or \
its sources needs re-verification. 6. Approval table with empty name, role, signature and date cells.",
        ),
        // Procedures.
        "procedure_list" => (
            "Procedure list",
            "List the procedures the system needs. Table with columns: ID (SOP-01, ...), Procedure, Purpose, Required \
by (regulation clause or company control), Exists already (yes, no, update; cite the source file where it exists), \
Owner role. Cover at least: system administration and user access, configuration and change control, backup and \
restore, business continuity and disaster recovery, audit trail review, incident and problem handling, periodic \
review, data archiving and retention, training, and daily use. Then note which procedures can be shared with other \
systems.",
        ),
        "administration" => (
            "System administration procedure",
            "Write the system administration SOP. Sections: 1. Purpose and scope. 2. Responsibilities by role. \
3. User access: requesting, approving, creating, changing and removing accounts, and the periodic access review, \
with the roles and permissions observed (cite). 4. Configuration control: who may change settings and how changes \
are recorded. 5. Audit trail review: what is reviewed, how often, and by whom. 6. Maintenance, vendor updates and \
patches. 7. Records this procedure creates. Write instructions as numbered steps naming the screens and fields \
observed; anything not observed is a [GAP: ...].",
        ),
        "backup" => (
            "Backup and recovery procedure",
            "Write the backup and recovery SOP. Sections: 1. Purpose and scope. 2. Responsibilities by role (including \
the vendor for hosted systems). 3. What is backed up, how often, how long backups are kept, and where. 4. Monitoring \
that backups succeed. 5. Restore: numbered steps, and the periodic restore test with its acceptance criteria. \
6. Business continuity: how work continues while the system is down. 7. Disaster recovery: recovery time and recovery \
point objectives, as the evidence gives them. 8. Records this procedure creates.",
        ),
        "training" => (
            "Training plan",
            "Write the training plan. Sections: 1. Roles that use the system (cite the observed roles). 2. Training \
matrix: table of role, training item (procedure, work instruction or system training), method (read and understood, \
classroom, hands-on), required before access (yes/no), refresher interval. 3. How training is recorded and checked \
before access is granted. 4. Training material needed and who writes it.",
        ),
        "work_instructions" => (
            "Work instructions",
            "Write step-by-step work instructions for the tasks observed. For each task, a heading that begins with its \
ID, for example '### WI-01 Log a sample', then the role that does it, prerequisites, and numbered steps naming the \
screen, field and button exactly as observed, each cited [O..]. Add notes on checks the user must make, values they \
must never share, and what to do if the system shows an error. Tasks that a procedure needs but were not observed \
are listed at the end as [GAP: ...].",
        ),
        _ => return None,
    };
    Some(DocumentSpec { title, instructions })
}

/// For learning a company's writing style from its own documents.
pub const STYLE_SYSTEM: &str = "You study a regulated company's computer-system validation documents and describe \
how the company writes them, so that new documents can be drafted in the same house style. Describe the style only. \
Never copy facts from the samples: no system names, people, results, dates or numbers that belong to one document. \
Write in plain English, as short Markdown bullet lists under these headings, and nothing else: \
## Voice and wording, ## Document structure, ## Requirements, ## Test scripts, ## IDs and numbering, \
## Level of detail, ## Terms the company uses. Under Test scripts, say how steps, expected results, actual results, \
evidence and pass/fail are written. Where the samples disagree or say nothing about a heading, say so in one bullet. \
Keep the whole guide under 450 words.";

pub fn style_user_prompt(samples: &[(String, String)]) -> String {
    let mut p = String::from("Describe the house style of these validation documents.\n");
    for (i, (name, text)) in samples.iter().enumerate() {
        p.push_str(&format!("\n# Sample {} ({name})\n{text}\n", i + 1));
    }
    p
}
