//! Regulations and guidance Peter knows about, summarised in Peter's own words
//! with their clause references. Not the official text: always check the
//! source for the exact wording.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Clause {
    pub reference: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Standard {
    pub id: &'static str,
    pub name: &'static str,
    pub issuer: &'static str,
    pub summary: &'static str,
    pub clauses: &'static [Clause],
}

const fn c(reference: &'static str, title: &'static str, summary: &'static str) -> Clause {
    Clause { reference, title, summary }
}

pub static CATALOG: &[Standard] = &[
    Standard {
        id: "part11",
        name: "21 CFR Part 11",
        issuer: "US FDA",
        summary: "When electronic records and electronic signatures are trusted as equivalent to paper records and handwritten signatures.",
        clauses: &[
            c("11.10(a)", "Validation", "Systems are validated so they are accurate, reliable and perform as intended, and invalid or altered records can be detected."),
            c("11.10(b)", "Copies for inspection", "Accurate and complete copies of records can be produced in human-readable and electronic form."),
            c("11.10(c)", "Record protection", "Records are protected so they can be retrieved accurately and readily for as long as they must be kept."),
            c("11.10(d)", "Access limits", "Only authorised people can access the system."),
            c("11.10(e)", "Audit trail", "Secure, computer-generated, time-stamped audit trails record who created, changed or deleted records and when; changes do not hide earlier entries; trails are kept as long as the records."),
            c("11.10(f)", "Operational checks", "The system enforces the permitted order of steps and events where order matters."),
            c("11.10(g)", "Authority checks", "Only authorised people can use the system, sign records, change records or perform the operation at hand."),
            c("11.10(h)", "Device checks", "Where it matters, the system checks that input comes from a valid source or terminal."),
            c("11.10(i)", "Training", "People who build, maintain or use the system have the education, training and experience for their tasks."),
            c("11.10(j)", "Accountability policy", "Written policies hold people accountable for actions taken under their electronic signatures."),
            c("11.10(k)", "System documentation control", "Distribution of, access to and changes to system documentation are controlled."),
            c("11.30", "Open systems", "Systems open to people outside the organisation add controls such as encryption to protect records."),
            c("11.50", "Signature manifestation", "Signed records show the signer's printed name, the date and time, and the meaning of the signature."),
            c("11.70", "Signature linking", "Signatures are linked to their records so they cannot be removed, copied or moved to falsify a record."),
            c("11.100", "Signature uniqueness", "Each electronic signature belongs to one person only and is never reused or reassigned; identity is verified first."),
            c("11.200", "Signature components", "Signatures not based on biometrics use at least two distinct parts, such as an ID and a password."),
            c("11.300", "ID and password controls", "IDs and passwords stay unique, are checked and renewed periodically, and lost or compromised credentials are handled."),
        ],
    },
    Standard {
        id: "annex11",
        name: "EU GMP Annex 11",
        issuer: "European Commission",
        summary: "Computerised systems used in GMP-regulated activities: risk-based validation and control throughout the system's life.",
        clauses: &[
            c("Principle", "Principle", "A computerised system replacing a manual one must not reduce product quality, process control or quality assurance."),
            c("1", "Risk management", "Apply risk management through the whole life cycle, considering patient safety, data integrity and product quality."),
            c("2", "Personnel", "Process owners, system owners, quality people and IT work together, with suitable training and defined responsibilities."),
            c("3", "Suppliers and service providers", "Formal agreements define responsibilities; suppliers are assessed according to risk."),
            c("4", "Validation", "Validation documents cover the life cycle, including user requirements, a system inventory, change control, data migration and test evidence."),
            c("5", "Data", "Systems that exchange data electronically have checks for correct and secure entry and processing."),
            c("6", "Accuracy checks", "Critical data entered manually is checked for accuracy."),
            c("7", "Data storage", "Data is secured against damage, kept readable and accessible, and backed up regularly with restores checked."),
            c("8", "Printouts", "Clear printed copies of electronic data can be obtained; printouts of batch release data show changes since entry."),
            c("9", "Audit trails", "Based on risk, the system records all GMP-relevant changes and deletions, with the reason documented."),
            c("10", "Change and configuration management", "Changes to systems and their configuration are made in a controlled way under a defined procedure."),
            c("11", "Periodic evaluation", "Systems are reviewed periodically to confirm they remain valid and compliant."),
            c("12", "Security", "Physical and logical controls restrict access to authorised people; creation, change and removal of access is recorded."),
            c("13", "Incident management", "Incidents, including system failures and data errors, are reported, assessed and their root causes addressed."),
            c("14", "Electronic signature", "Electronic signatures carry the same weight as handwritten ones, are permanently linked to their records, and include the time and date."),
            c("15", "Batch release", "Only qualified persons can certify batch release in the system, and the system identifies who did it."),
            c("16", "Business continuity", "Arrangements keep critical processes running if a system fails, and are tested."),
            c("17", "Archiving", "Archived data is checked for accessibility, readability and integrity, including after system changes."),
        ],
    },
    Standard {
        id: "gamp5",
        name: "GAMP 5 software categories",
        issuer: "ISPE (guidance)",
        summary: "A risk-based life-cycle approach that scales validation effort to the kind of software and its impact. Summarised here: the categories only.",
        clauses: &[
            c("Category 1", "Infrastructure software", "Operating systems, databases, middleware and tools. Record the version and verify installation; little or no functional testing."),
            c("Category 3", "Non-configured products", "Off-the-shelf software used as delivered, with only run-time parameters set. Test against user requirements."),
            c("Category 4", "Configured products", "Off-the-shelf software configured to the business process. Document and test the configuration and the requirements it meets."),
            c("Category 5", "Custom applications", "Software written or customised for the company. Full life cycle: specifications, design review, code review and testing."),
            c("Supplier leverage", "Using supplier work", "Supplier documentation and testing can be used to reduce the company's own effort, after assessing the supplier."),
        ],
    },
    Standard {
        id: "alcoa",
        name: "ALCOA+ data integrity principles",
        issuer: "Regulators' data integrity guidance",
        summary: "The qualities GxP data must have throughout its life.",
        clauses: &[
            c("A", "Attributable", "It is clear who recorded or changed the data, and when."),
            c("L", "Legible", "The data can be read and understood for as long as it is kept."),
            c("C", "Contemporaneous", "Data is recorded at the time the activity is performed."),
            c("O", "Original", "The first record, or a certified true copy, is kept."),
            c("A", "Accurate", "Data is correct, and any changes are documented."),
            c("+C", "Complete", "All data is kept, including repeats and failed runs."),
            c("+C", "Consistent", "Records follow the expected sequence, with consistent dates and times."),
            c("+E", "Enduring", "Data lasts for the whole retention period on suitable media."),
            c("+A", "Available", "Data can be retrieved for review and inspection when needed."),
        ],
    },
    Standard {
        id: "ichq9",
        name: "ICH Q9(R1) Quality risk management",
        issuer: "ICH",
        summary: "A systematic approach to assessing, controlling, communicating and reviewing risks to quality.",
        clauses: &[
            c("Principle 1", "Patient protection", "Risk evaluation rests on scientific knowledge and ultimately links to protecting the patient."),
            c("Principle 2", "Proportionate effort", "The effort, formality and documentation match the level of risk."),
            c("Assessment", "Risk assessment", "Identify hazards, analyse the risk of each, and evaluate it against criteria."),
            c("Control", "Risk control", "Reduce risks to an acceptable level, and document the decision to accept any that remain."),
            c("Communication", "Risk communication", "Share risk information and decisions with the people who need them."),
            c("Review", "Risk review", "Review risks over time as knowledge and experience grow."),
            c("Subjectivity", "Managing subjectivity", "Use defined scales and well-informed people to reduce subjectivity in risk scoring."),
        ],
    },
];

pub fn find(id: &str) -> Option<&'static Standard> {
    CATALOG.iter().find(|s| s.id == id)
}

/// The prompt section listing the regulations in scope.
pub fn prompt_section(ids: &[String]) -> String {
    let chosen: Vec<&Standard> = ids.iter().filter_map(|id| find(id)).collect();
    if chosen.is_empty() {
        return String::new();
    }
    let mut p = String::from(
        "\n# Regulations in scope\nThe company works to the regulations below (summaries, not the official text). Where a \
statement addresses one, name the clause in the text, for example \"(21 CFR 11.10(e))\" or \"(Annex 11, clause 9)\". \
These are references, not evidence: still cite observed steps, sources and documents for facts.\n",
    );
    for s in chosen {
        p.push_str(&format!("\n## {} ({})\n", s.name, s.issuer));
        for c in s.clauses {
            p.push_str(&format!("- {} {}: {}\n", c.reference, c.title, c.summary));
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique_and_prompt_lists_choices() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|s| s.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        assert_eq!(prompt_section(&[]), "");
        let p = prompt_section(&["part11".into(), "unknown".into()]);
        assert!(p.contains("## 21 CFR Part 11 (US FDA)") && p.contains("- 11.10(e) Audit trail:"));
        assert!(!p.contains("Annex 11 ("));
    }
}
