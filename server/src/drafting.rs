//! Drafting a step's document: gather the session's evidence, ask the chosen
//! provider, check the citations, and keep the result as a new version.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    ai::{self, Prompt},
    catalog, extract, prompts,
    security::{new_id, now},
    state::{AppState, SessionEvent},
};

/// Bounds that keep a prompt to a sensible size.
const MAX_OBSERVATIONS: usize = 400;
const MAX_SOURCE_CHARS: usize = 12_000;
const MAX_ALL_SOURCE_CHARS: usize = 48_000;
const MAX_DOCUMENT_CHARS: usize = 8_000;
const MAX_TEMPLATE_CHARS: usize = 10_000;
const MAX_CONTROLS: usize = 200;
const MAX_STYLE_CHARS: usize = 6_000;
const MAX_ALL_BASELINE_CHARS: usize = 36_000;

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SourceRef {
    pub id: String,
    pub filename: String,
}

/// Everything a draft may cite.
pub struct Evidence {
    pub project: Option<(String, String, String, String)>, // name, system, vendor, description
    pub session_title: String,
    pub type_title: String,
    pub observations: Vec<ObservationLine>,
    pub sources: Vec<(String, SourceRef, Result<String, String>)>, // ("S1", ref, text)
    pub documents: Vec<(String, String, String)>,                  // (step_id, title, body)
    /// Regulations the workspace selected in the Library.
    pub standards: Vec<String>,
    /// The company's template for this step, if one is in the Library: (name, text).
    pub template: Option<(String, String)>,
    /// The project's SOP controls: (CTL-001, statement).
    pub controls: Vec<(String, String)>,
    /// The company's writing style guide, from the Library.
    pub style: Option<String>,
    /// The earlier validation this session builds on, if one is linked.
    pub baseline: Option<Baseline>,
}

/// An earlier session's documents, cited as [B:step].
pub struct Baseline {
    pub session_id: String,
    pub title: String,
    pub type_title: String,
    pub documents: Vec<(String, String, String)>, // (step_id, title, body)
}

pub struct ObservationLine {
    pub seq: i64,
    pub text: String,
}

#[derive(sqlx::FromRow)]
struct ObsRow {
    seq: i64,
    kind: String,
    url: String,
    page_title: String,
    target_label: String,
    value: String,
    masked: bool,
    note: String,
    captured_at_ms: i64,
    created_by_name: String,
}

fn describe(o: &ObsRow) -> String {
    let place = if o.page_title.is_empty() { o.url.clone() } else { format!("{} ({})", o.page_title, o.url) };
    let what = match o.kind.as_str() {
        "navigate" => format!("opened page {place}"),
        "click" => format!("clicked \"{}\" on {place}", o.target_label),
        "input" if o.masked => format!("entered a hidden value in \"{}\" on {place}", o.target_label),
        "input" => format!("entered \"{}\" in \"{}\" on {place}", o.value, o.target_label),
        "select" => format!("chose \"{}\" in \"{}\" on {place}", o.value, o.target_label),
        "submit" => format!("submitted form \"{}\" on {place}", o.target_label),
        "note" => format!("tester's note: {}", o.note),
        "capture" => format!("took a screenshot of {place}"),
        other => format!("{other} on {place}"),
    };
    let secs = o.captured_at_ms / 1000;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{what} — by {} at {h:02}:{m:02}:{s:02} UTC", o.created_by_name)
}

pub async fn gather(state: &AppState, session_id: &str, step_id: &str) -> Result<Evidence, String> {
    let db_err = |e: sqlx::Error| format!("Could not read the session: {e}");
    let (title, validation_type, project_id, workspace_id, baseline_id): (String, String, Option<String>, String, Option<String>) =
        sqlx::query_as("SELECT title, validation_type, project_id, workspace_id, baseline_session_id FROM validation_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_one(&state.db)
            .await
            .map_err(db_err)?;
    let kind = catalog::find(&validation_type).ok_or("Unknown validation type.")?;

    let project = match &project_id {
        Some(pid) => sqlx::query_as("SELECT name, system_name, vendor, description FROM projects WHERE id = ?")
            .bind(pid)
            .fetch_optional(&state.db)
            .await
            .map_err(db_err)?,
        None => None,
    };

    let mut rows: Vec<ObsRow> = sqlx::query_as(
        "SELECT o.seq, o.kind, o.url, o.page_title, o.target_label, o.value, o.masked, o.note, o.captured_at_ms,
                u.name AS created_by_name
         FROM observations o JOIN users u ON u.id = o.created_by
         WHERE o.session_id = ? AND o.deleted_at IS NULL ORDER BY o.seq DESC LIMIT ?",
    )
    .bind(session_id)
    .bind(MAX_OBSERVATIONS as i64)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    rows.reverse();
    let observations = rows.iter().map(|o| ObservationLine { seq: o.seq, text: describe(o) }).collect();

    // Source numbers follow upload order, and each draft records what they meant.
    let mut sources = Vec::new();
    if let Some(pid) = &project_id {
        let files: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, filename, content_type FROM project_sources
             WHERE project_id = ? AND deleted_at IS NULL ORDER BY created_at, id",
        )
        .bind(pid)
        .fetch_all(&state.db)
        .await
        .map_err(db_err)?;
        let mut budget = MAX_ALL_SOURCE_CHARS;
        for (i, (id, filename, content_type)) in files.into_iter().enumerate() {
            let bytes = crate::storage::get(state, "sources", &id).await.map_err(|_| "Could not read a source file.".to_string())?;
            let text = match bytes {
                Some(b) => extract::text_of(b, content_type).await,
                None => Err("the stored file is missing".to_string()),
            };
            let text = text.map(|t| {
                let t = extract::tidy(&t);
                let take = MAX_SOURCE_CHARS.min(budget);
                let cut: String = t.chars().take(take).collect();
                budget = budget.saturating_sub(cut.chars().count());
                if cut.chars().count() < t.chars().count() { format!("{cut}\n[... excerpt ends here ...]") } else { cut }
            });
            sources.push((format!("S{}", i + 1), SourceRef { id, filename }, text));
        }
    }

    // Documents from earlier steps of this session, in step order.
    let position = kind.steps.iter().position(|s| s.id == step_id).unwrap_or(kind.steps.len());
    let earlier: BTreeSet<&str> = kind.steps[..position].iter().map(|s| s.id).collect();
    let docs: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT d.step_id, d.title, v.body FROM documents d
         JOIN document_versions v ON v.document_id = d.id AND v.version = d.current_version
         WHERE d.session_id = ?",
    )
    .bind(session_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    let mut documents: Vec<(String, String, String)> = docs
        .into_iter()
        .filter(|(step, _, _)| earlier.contains(step.as_str()))
        .map(|(step, t, body)| {
            let body: String = body.chars().take(MAX_DOCUMENT_CHARS).collect();
            (step, t, body)
        })
        .collect();
    documents.sort_by_key(|(step, _, _)| kind.steps.iter().position(|s| s.id == step));

    let mut standards: Vec<String> = sqlx::query_as::<_, (String,)>("SELECT standard_id FROM workspace_standards WHERE workspace_id = ?")
        .bind(&workspace_id)
        .fetch_all(&state.db)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|(id,)| id)
        .collect();
    // In catalog order (Part 11 first), not alphabetical.
    standards.sort_by_key(|id| crate::standards::CATALOG.iter().position(|s| s.id == id));
    let template: Option<(String, String)> = sqlx::query_as(
        "SELECT name, text FROM library_templates WHERE workspace_id = ? AND step_id = ? AND archived_at IS NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&workspace_id)
    .bind(step_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_err)?;

    let controls: Vec<(String, String)> = match &project_id {
        Some(pid) => sqlx::query_as::<_, (i64, String)>(
            "SELECT number, text FROM project_controls WHERE project_id = ? AND archived_at IS NULL ORDER BY number LIMIT ?",
        )
        .bind(pid)
        .bind(MAX_CONTROLS as i64)
        .fetch_all(&state.db)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|(n, text)| (crate::controls::reference(n), text.chars().take(300).collect()))
        .collect(),
        None => Vec::new(),
    };

    let style: Option<String> = sqlx::query_as::<_, (String,)>("SELECT guide FROM workspace_style WHERE workspace_id = ?")
        .bind(&workspace_id)
        .fetch_optional(&state.db)
        .await
        .map_err(db_err)?
        .map(|(g,)| g.chars().take(MAX_STYLE_CHARS).collect())
        .filter(|g: &String| !g.trim().is_empty());

    let baseline = match baseline_id {
        Some(bid) => load_baseline(state, &bid).await.map_err(db_err)?,
        None => None,
    };

    Ok(Evidence {
        project,
        session_title: title,
        type_title: kind.title.to_string(),
        observations,
        sources,
        documents,
        standards,
        template,
        controls,
        style,
        baseline,
    })
}

/// The current version of each document in the earlier session, in step order.
async fn load_baseline(state: &AppState, session_id: &str) -> Result<Option<Baseline>, sqlx::Error> {
    let row: Option<(String, String)> = sqlx::query_as("SELECT title, validation_type FROM validation_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_optional(&state.db)
        .await?;
    let Some((title, type_id)) = row else { return Ok(None) };
    let kind = catalog::find(&type_id);
    let mut docs: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT d.step_id, d.title, v.body FROM documents d
         JOIN document_versions v ON v.document_id = d.id AND v.version = d.current_version
         WHERE d.session_id = ?",
    )
    .bind(session_id)
    .fetch_all(&state.db)
    .await?;
    docs.sort_by_key(|(step, _, _)| kind.and_then(|k| k.steps.iter().position(|s| s.id == step)).unwrap_or(usize::MAX));
    let mut budget = MAX_ALL_BASELINE_CHARS;
    let documents = docs
        .into_iter()
        .map(|(step, t, body)| {
            let take = MAX_DOCUMENT_CHARS.min(budget);
            let cut: String = body.chars().take(take).collect();
            budget = budget.saturating_sub(cut.chars().count());
            let body = if cut.chars().count() < body.chars().count() { format!("{cut}\n[... excerpt ends here ...]") } else { cut };
            (step, t, body)
        })
        .collect();
    Ok(Some(Baseline {
        session_id: session_id.to_string(),
        title,
        type_title: kind.map(|k| k.title).unwrap_or("Validation").to_string(),
        documents,
    }))
}

pub fn user_prompt(evidence: &Evidence, spec: &prompts::DocumentSpec, extra: &str) -> String {
    let mut p = String::new();
    p.push_str(&format!("# Task\nWrite the {} for this {}.\n\n{}\n", spec.title, evidence.type_title.to_lowercase(), spec.instructions));
    if !extra.trim().is_empty() {
        p.push_str(&format!("\nAdditional instructions from the validation lead:\n{}\n", extra.trim()));
    }
    if let Some((name, text)) = &evidence.template {
        let text: String = text.chars().take(MAX_TEMPLATE_CHARS).collect();
        p.push_str(&format!(
            "\n# Company template\nThe company writes this document using its template \"{name}\". Follow its section \
headings, their order and its table columns, and keep its wording for fixed text such as purpose statements. \
Placeholders in the template are not evidence. Template text:\n\n{text}\n"
        ));
    }
    if let Some(style) = &evidence.style {
        p.push_str(&format!(
            "\n# Company writing style\nWrite in the company's house style, described below. It decides wording, \
structure and level of detail only: the rules on evidence, citations and gaps always come first.\n\n{style}\n"
        ));
    }
    p.push_str(&crate::standards::prompt_section(&evidence.standards));
    if !evidence.controls.is_empty() {
        p.push_str(
            "\n# Company controls\nControls the company's SOPs require, from the project's Controls list. Where a requirement \
meets one, name its ID in the requirement's row, for example \"(CTL-003)\", because Peter reads these IDs to show which \
controls are covered. Controls are not evidence: still cite observed steps, sources and documents for facts.\n",
        );
        for (id, text) in &evidence.controls {
            p.push_str(&format!("- {id}: {text}\n"));
        }
    }
    p.push_str("\n# System under validation\n");
    p.push_str(&format!("Session: {}\n", evidence.session_title));
    match &evidence.project {
        Some((name, system, vendor, description)) => {
            p.push_str(&format!("Project: {name}\n"));
            if !system.is_empty() {
                p.push_str(&format!("System and version: {system}\n"));
            }
            if !vendor.is_empty() {
                p.push_str(&format!("Vendor: {vendor}\n"));
            }
            if !description.is_empty() {
                p.push_str(&format!("Intended use, as stated by the project: {description}\n"));
            }
        }
        None => p.push_str("Project: none (the session is not filed in a project, so there are no source files)\n"),
    }

    p.push_str("\n# Evidence\n\n## Observed steps\n");
    if evidence.observations.is_empty() {
        p.push_str("None recorded.\n");
    }
    for o in &evidence.observations {
        p.push_str(&format!("[O{}] {}\n", o.seq, o.text));
    }

    p.push_str("\n## Source files\n");
    if evidence.sources.is_empty() {
        p.push_str("None.\n");
    }
    for (tag, src, text) in &evidence.sources {
        match text {
            Ok(t) if !t.trim().is_empty() => p.push_str(&format!("\n### [{tag}] {}\n{t}\n", src.filename)),
            Ok(_) => p.push_str(&format!("\n### [{tag}] {}\n(no text in this file)\n", src.filename)),
            Err(reason) => p.push_str(&format!("\n### [{tag}] {}\n(contents not available: {reason}; cite only its name)\n", src.filename)),
        }
    }

    p.push_str("\n## Earlier documents in this session\n");
    if evidence.documents.is_empty() {
        p.push_str("None yet.\n");
    }
    for (step, title, body) in &evidence.documents {
        p.push_str(&format!("\n### [D:{step}] {title}\n{body}\n"));
    }

    p.push_str("\n## Earlier validation this session builds on\n");
    match &evidence.baseline {
        None => p.push_str("None linked.\n"),
        Some(b) if b.documents.is_empty() => {
            p.push_str(&format!("{} \"{}\", which has no documents yet.\n", b.type_title, b.title));
        }
        Some(b) => {
            p.push_str(&format!("{} \"{}\". Cite its documents as [B:step].\n", b.type_title, b.title));
            for (step, title, body) in &b.documents {
                p.push_str(&format!("\n### [B:{step}] {title}\n{body}\n"));
            }
        }
    }
    p
}

/// Which evidence ids a document cites, which cited ids do not exist, and how
/// many gaps it flags.
pub fn check_citations(body: &str, known: &BTreeSet<String>) -> (Vec<String>, Vec<String>, usize) {
    let mut cited = BTreeSet::new();
    let mut unknown = BTreeSet::new();
    let mut gaps = 0;
    let mut rest = body;
    while let Some(start) = rest.find('[') {
        let after = &rest[start + 1..];
        let Some(end) = after.find(']') else { break };
        let inner = after[..end].trim();
        if inner.starts_with("GAP") {
            gaps += 1;
        } else if is_citation(inner) {
            if known.contains(inner) {
                cited.insert(inner.to_string());
            } else {
                unknown.insert(inner.to_string());
            }
        }
        rest = &after[end..];
    }
    (cited.into_iter().collect(), unknown.into_iter().collect(), gaps)
}

fn is_citation(s: &str) -> bool {
    let digits = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    s.strip_prefix('O').is_some_and(digits)
        || s.strip_prefix('S').is_some_and(digits)
        || s.strip_prefix("D:").is_some_and(step_name)
        || s.strip_prefix("B:").is_some_and(step_name)
}

fn step_name(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

pub fn known_ids(evidence: &Evidence) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    ids.extend(evidence.observations.iter().map(|o| format!("O{}", o.seq)));
    ids.extend(evidence.sources.iter().map(|(tag, _, _)| tag.clone()));
    ids.extend(evidence.documents.iter().map(|(step, _, _)| format!("D:{step}")));
    if let Some(b) = &evidence.baseline {
        ids.extend(b.documents.iter().map(|(step, _, _)| format!("B:{step}")));
    }
    ids
}

/// What a version's citations meant when it was saved: the source numbers,
/// and the earlier validation its [B:step] citations point at.
pub fn citations_json(evidence: &Evidence, cited: &[String], unknown: &[String], gaps: usize) -> Value {
    let sources: BTreeMap<String, SourceRef> = evidence.sources.iter().map(|(tag, r, _)| (tag.clone(), r.clone())).collect();
    let mut v = json!({ "cited": cited, "unknown": unknown, "gaps": gaps, "sources": sources });
    if let Some(b) = &evidence.baseline {
        let titles: BTreeMap<&str, &str> = b.documents.iter().map(|(step, t, _)| (step.as_str(), t.as_str())).collect();
        v["baseline"] = json!({ "session_id": b.session_id, "title": b.title, "documents": titles });
    }
    v
}

/// A draft built from the evidence without AI, for trying Peter without a key.
fn sample_draft(evidence: &Evidence, spec: &prompts::DocumentSpec) -> String {
    let mut d = format!(
        "# {}\n\n> Sample draft made by Peter without AI. It lists the evidence this document would draw on. \
Choose an AI provider to draft the real text.\n\n## Evidence available\n\n",
        spec.title
    );
    if evidence.observations.is_empty() {
        d.push_str("No steps have been observed yet. [GAP: record the system with the Peter Observer extension]\n");
    } else {
        d.push_str("| Step | What happened |\n| --- | --- |\n");
        for o in evidence.observations.iter().take(60) {
            d.push_str(&format!("| {} | {} [O{}] |\n", o.seq, o.text.replace('|', "/"), o.seq));
        }
    }
    d.push_str("\n## Source files\n\n");
    if evidence.sources.is_empty() {
        d.push_str("None. [GAP: add vendor manuals, SOPs or specifications to the project]\n");
    }
    for (tag, src, text) in &evidence.sources {
        let status = match text {
            Ok(t) => format!("{} characters of text", t.chars().count()),
            Err(reason) => reason.clone(),
        };
        d.push_str(&format!("- {} ({status}) [{tag}]\n", src.filename));
    }
    if !evidence.documents.is_empty() {
        d.push_str("\n## Earlier documents\n\n");
        for (step, title, _) in &evidence.documents {
            d.push_str(&format!("- {title} [D:{step}]\n"));
        }
    }
    if let Some(b) = &evidence.baseline {
        d.push_str(&format!("\n## Earlier validation\n\nThis session builds on {} \"{}\".\n\n", b.type_title.to_lowercase(), b.title));
        if b.documents.is_empty() {
            d.push_str("It has no documents yet. [GAP: draft the earlier validation's documents, or link another session]\n");
        }
        for (step, title, _) in &b.documents {
            d.push_str(&format!("- {title} [B:{step}]\n"));
        }
    }
    if let Some((name, _)) = &evidence.template {
        d.push_str(&format!("\n## Company template\n\nDrafts follow the company template \"{name}\" from the Library.\n"));
    }
    if evidence.style.is_some() {
        d.push_str("\n## Writing style\n\nDrafts follow the company's writing style from the Library.\n");
    }
    if !evidence.controls.is_empty() {
        d.push_str(&format!("\n## Company controls\n\n{} controls from the project's SOPs: {}\n", evidence.controls.len(),
            evidence.controls.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(", ")));
    }
    if !evidence.standards.is_empty() {
        let names: Vec<&str> = evidence.standards.iter().filter_map(|id| crate::standards::find(id).map(|s| s.name)).collect();
        d.push_str(&format!("\n## Regulations in scope\n\n{}\n", names.join(", ")));
    }
    d.push_str(&format!("\n## What this document must cover\n\n{}\n\n[GAP: draft with an AI provider]\n", spec.instructions));
    d
}

/// Strips a ```markdown fence if a model wrapped the whole answer in one.
fn unwrap_fence(text: &str) -> String {
    let t = text.trim();
    if let Some(inner) = t.strip_prefix("```").and_then(|r| r.split_once('\n')).map(|(_, body)| body)
        && let Some(body) = inner.trim_end().strip_suffix("```")
    {
        return body.trim().to_string();
    }
    t.to_string()
}

pub struct DraftJob {
    pub workspace_id: String,
    pub session_id: String,
    pub step_id: String,
    pub document_id: String,
    pub user_id: String,
    pub provider: String,
    pub extra: String,
}

fn publish(state: &AppState, job: &DraftJob, data: Value) {
    let _ = state.events.send(SessionEvent {
        workspace_id: job.workspace_id.clone(),
        session_id: job.session_id.clone(),
        name: "document",
        data,
    });
}

/// Runs a draft to the end and records the outcome. Never panics the server.
pub async fn run(state: AppState, job: DraftJob) {
    let outcome = draft(&state, &job).await;
    let t = now();
    let result: Result<(), sqlx::Error> = async {
        let mut tx = state.db.begin().await?;
        match &outcome {
            Ok((version, provider_model, summary)) => {
                sqlx::query("UPDATE documents SET status = 'ready', error = NULL, current_version = ?, updated_at = ? WHERE id = ?")
                    .bind(version)
                    .bind(t)
                    .bind(&job.document_id)
                    .execute(&mut *tx)
                    .await?;
                crate::auth::audit(&mut tx, Some(&job.workspace_id), Some(&job.user_id), "document.drafted", Some(&job.document_id),
                    json!({ "session_id": job.session_id, "step_id": job.step_id, "version": version,
                            "provider": job.provider, "model": provider_model, "citations": summary }))
                    .await
                    .map_err(|_| sqlx::Error::Protocol("audit failed".into()))?;
            }
            Err(message) => {
                sqlx::query("UPDATE documents SET status = 'failed', error = ?, updated_at = ? WHERE id = ?")
                    .bind(message)
                    .bind(t)
                    .bind(&job.document_id)
                    .execute(&mut *tx)
                    .await?;
                crate::auth::audit(&mut tx, Some(&job.workspace_id), Some(&job.user_id), "document.draft_failed", Some(&job.document_id),
                    json!({ "session_id": job.session_id, "step_id": job.step_id, "provider": job.provider, "error": message }))
                    .await
                    .map_err(|_| sqlx::Error::Protocol("audit failed".into()))?;
            }
        }
        tx.commit().await
    }
    .await;
    if let Err(e) = result {
        tracing::error!(error = %e, document_id = %job.document_id, "could not record the draft outcome");
    }
    let status = if outcome.is_ok() { "ready" } else { "failed" };
    publish(&state, &job, json!({ "step_id": job.step_id, "status": status }));
}

/// Returns the new version number, the model used, and a citation summary.
async fn draft(state: &AppState, job: &DraftJob) -> Result<(i64, String, Value), String> {
    let spec = prompts::spec(&job.step_id).ok_or("This step has no document to draft.")?;
    let evidence = gather(state, &job.session_id, &job.step_id).await?;
    let completion = if job.provider == "sample" {
        ai::Completion {
            text: sample_draft(&evidence, &spec),
            model: "peter-sample".into(),
            prompt_tokens: None,
            completion_tokens: None,
            cost_usd: None,
        }
    } else {
        let user = user_prompt(&evidence, &spec, &job.extra);
        let effective = crate::workspace_ai::effective(state, &job.workspace_id).await?;
        let source = effective.source(&job.provider).ok_or("That AI provider has no key for this workspace.")?;
        let result = ai::complete(&effective.config, &job.provider, Prompt { system: prompts::SYSTEM, user: &user }).await;
        crate::workspace_ai::record(state, job.into(), source, &result).await;
        result?
    };
    let body = unwrap_fence(&completion.text);
    let known = known_ids(&evidence);
    let (cited, unknown, gaps) = check_citations(&body, &known);
    let citations = citations_json(&evidence, &cited, &unknown, gaps);

    let version: i64 = {
        let (current,): (i64,) = sqlx::query_as("SELECT current_version FROM documents WHERE id = ?")
            .bind(&job.document_id)
            .fetch_one(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        current + 1
    };
    sqlx::query(
        "INSERT INTO document_versions
         (id, document_id, version, body, origin, provider, model, prompt_tokens, completion_tokens, citations, created_by, created_at)
         VALUES (?, ?, ?, ?, 'ai', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(&job.document_id)
    .bind(version)
    .bind(&body)
    .bind(&job.provider)
    .bind(&completion.model)
    .bind(completion.prompt_tokens)
    .bind(completion.completion_tokens)
    .bind(citations.to_string())
    .bind(&job.user_id)
    .bind(now())
    .execute(&state.db)
    .await
    .map_err(|e| format!("Could not save the draft: {e}"))?;
    let summary = json!({ "cited": cited.len(), "unknown": unknown.len(), "gaps": gaps });
    Ok((version, completion.model, summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations_are_sorted_into_known_unknown_and_gaps() {
        let known: BTreeSet<String> = ["O1", "O2", "S1", "D:requirements", "B:testing"].iter().map(|s| s.to_string()).collect();
        let body = "Users sign in [O1][O2]. Per manual [S1] and [D:requirements]. Made up [O99] [S7]. \
                    Backups [GAP: no backup SOP]. A [link](x) and [not a citation] and [GAP: second]. \
                    Baseline tests [B:testing], missing [B:plan].";
        let (cited, unknown, gaps) = check_citations(body, &known);
        assert_eq!(cited, ["B:testing", "D:requirements", "O1", "O2", "S1"]);
        assert_eq!(unknown, ["B:plan", "O99", "S7"]);
        assert_eq!(gaps, 2);
    }

    #[test]
    fn fences_around_the_whole_answer_are_removed() {
        assert_eq!(unwrap_fence("```markdown\n# Title\nText\n```"), "# Title\nText");
        assert_eq!(unwrap_fence("# Title\n```\ncode\n```"), "# Title\n```\ncode\n```");
    }
}
