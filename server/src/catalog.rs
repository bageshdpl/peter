//! The kinds of validation work Peter guides, and the steps of each.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Step {
    pub id: &'static str,
    pub label: &'static str,
    pub summary: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ValidationType {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    /// Whether sessions of this type can be started yet.
    pub available: bool,
    pub steps: &'static [Step],
}

const fn step(id: &'static str, label: &'static str, summary: &'static str) -> Step {
    Step { id, label, summary }
}

pub static TYPES: &[ValidationType] = &[
    ValidationType {
        id: "initial",
        title: "Initial validation",
        summary: "Validate a system before its first use in GxP work.",
        available: true,
        steps: &[
            step("observe", "Observe and gather sources", "Record the system in use and collect vendor and site documents."),
            step("assessment", "System assessment", "Intended use, GxP impact and software category."),
            step("requirements", "Requirements and risk", "What the system must do, and the risk of each requirement failing."),
            step("specification", "Functional and design specification", "How the system meets the requirements, for custom builds."),
            step("configuration", "Configuration specification", "Settings, roles and workflows as configured."),
            step("plan", "Validation plan", "Scope, approach and responsibilities, scaled to risk."),
            step("installation", "Installation qualification", "Evidence the system is installed as specified."),
            step("testing", "Testing", "Test scripts and their executed results."),
            step("traceability", "Traceability matrix", "Each requirement linked to its tests and evidence."),
            step("summary", "Validation summary report", "Outcome, deviations and release decision."),
        ],
    },
    ValidationType {
        id: "change",
        title: "Change validation",
        summary: "Assess and test a change to a validated system.",
        available: false,
        steps: &[
            step("change_description", "Describe the change", "Configuration, new function, upgrade or move."),
            step("baseline", "Baseline documents", "The validated documents the change is measured against."),
            step("impact", "Impact assessment", "What the change affects and how much it matters."),
            step("test_scope", "Test scope", "Which tests to repeat or add."),
            step("document_updates", "Document updates", "Revise only the documents the change affects."),
            step("change_summary", "Change summary", "Results and release decision."),
        ],
    },
    ValidationType {
        id: "periodic_review",
        title: "Periodic review",
        summary: "Confirm a validated system is still in a validated state.",
        available: false,
        steps: &[
            step("history", "Changes and incidents", "What happened to the system since the last review."),
            step("controls", "Controls check", "Access, backups, audit trail and procedures still working."),
            step("review_report", "Review report", "Conclusion and actions."),
        ],
    },
    ValidationType {
        id: "data_migration",
        title: "Data migration",
        summary: "Move GxP data between systems without loss or change.",
        available: false,
        steps: &[
            step("migration_plan", "Migration plan", "Scope, method and acceptance criteria."),
            step("mapping", "Data mapping", "Each source field to its target field."),
            step("verification_plan", "Verification plan", "How completeness and accuracy will be checked."),
            step("migration_report", "Migration report", "Results and any discrepancies."),
        ],
    },
    ValidationType {
        id: "retirement",
        title: "Retirement",
        summary: "Take a validated system out of use and keep its records.",
        available: false,
        steps: &[
            step("retirement_plan", "Retirement plan", "Reason, timing and replacement."),
            step("archive_plan", "Archive plan", "Where records go, how long they are kept, and how to read them."),
            step("decommission", "Decommissioning", "Checklist for switching off and removing access."),
            step("retirement_report", "Retirement report", "Confirmation that everything was done."),
        ],
    },
    ValidationType {
        id: "ai",
        title: "AI validation",
        summary: "Validate the machine-learning part of a system.",
        available: false,
        steps: &[
            step("ai_use", "Intended use", "The decision the model supports and who relies on it."),
            step("ai_risk", "Context and risk", "Where errors would matter and how they would be caught."),
            step("ai_data", "Data and model", "Training data, model version and known limits."),
            step("ai_acceptance", "Acceptance criteria", "Measurable performance targets."),
            step("ai_testing", "Performance testing", "Results against the targets."),
            step("ai_monitoring", "Monitoring", "How performance is watched once in use."),
        ],
    },
    ValidationType {
        id: "equipment",
        title: "Equipment",
        summary: "Qualify a lab instrument, piece of equipment or utility.",
        available: false,
        steps: &[
            step("equipment_assessment", "Equipment assessment", "What it is, what it touches and its group."),
            step("equipment_qualification", "Qualification", "Installation, operational and performance checks for its group."),
        ],
    },
    ValidationType {
        id: "reports",
        title: "Dashboards and reports",
        summary: "Validate a report or dashboard people make decisions from.",
        available: false,
        steps: &[
            step("report_use", "Use assessment", "Who acts on it and the risk if it is wrong."),
            step("report_verification", "Verification", "Checks that the figures match their source data."),
        ],
    },
    ValidationType {
        id: "procedures",
        title: "Procedures",
        summary: "Write the SOPs and work instructions a system needs.",
        available: false,
        steps: &[
            step("procedure_list", "Procedure list", "Which procedures this system needs."),
            step("administration", "Administration", "User access, configuration control and maintenance."),
            step("backup", "Backup and recovery", "Backups, restore tests and business continuity."),
            step("training", "Training", "Who needs which training before using the system."),
            step("work_instructions", "Work instructions", "Step-by-step tasks for daily use."),
        ],
    },
];

pub fn find(id: &str) -> Option<&'static ValidationType> {
    TYPES.iter().find(|t| t.id == id)
}
