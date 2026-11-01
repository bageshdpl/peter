import { useCallback, useEffect, useRef, useState } from "react";
import { api, atLeast, fetchBlobUrl, formatTime, uploadFile } from "../api";
import { useSession } from "../App";
import { WritingStyle } from "../style";
import { messageOf, Notice, useAction } from "../ui";

type Tab = "regulations" | "templates" | "style" | "branding";

export default function Library() {
  const session = useSession()!;
  const isAdmin = atLeast(session.workspace.role, "admin");
  const [tab, setTab] = useState<Tab>("regulations");
  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.workspace.name}</p>
          <h1>Library</h1>
          <p className="muted">What Peter uses for every draft and export in this workspace.{!isAdmin && " Admins can change it."}</p>
        </div>
      </header>
      <div className="tabs" role="tablist">
        <button role="tab" aria-selected={tab === "regulations"} onClick={() => setTab("regulations")}>Regulations</button>
        <button role="tab" aria-selected={tab === "templates"} onClick={() => setTab("templates")}>Templates</button>
        <button role="tab" aria-selected={tab === "style"} onClick={() => setTab("style")}>Writing style</button>
        <button role="tab" aria-selected={tab === "branding"} onClick={() => setTab("branding")}>Branding</button>
      </div>
      {tab === "regulations" && <Regulations canEdit={isAdmin} />}
      {tab === "templates" && <Templates canEdit={isAdmin} />}
      {tab === "style" && <WritingStyle canEdit={isAdmin} />}
      {tab === "branding" && <Branding canEdit={isAdmin} />}
    </div>
  );
}

// ---------- Regulations ----------

interface Standard { id: string; name: string; issuer: string; summary: string; clauses: { reference: string; title: string; summary: string }[] }

function Regulations({ canEdit }: { canEdit: boolean }) {
  const [data, setData] = useState<{ standards: Standard[]; selected: string[] } | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const { busy, error, run } = useAction();
  useEffect(() => {
    api<{ standards: Standard[]; selected: string[] }>("GET", "/v1/library/standards").then(setData).catch(() => {});
  }, []);
  if (!data) return <p className="muted">Loading…</p>;

  const toggle = (id: string) =>
    run(async () => {
      const selected = data.selected.includes(id) ? data.selected.filter((s) => s !== id) : [...data.selected, id];
      const res = await api<{ selected: string[] }>("PUT", "/v1/library/standards", { selected });
      setData({ ...data, selected: res.selected });
    });

  return (
    <section className="card">
      <h2>Regulations in scope</h2>
      <p className="muted">
        Drafts name the clauses of the regulations you choose, for example “(21 CFR 11.10(e))”. These are Peter's own
        summaries for orientation, not the official text.
      </p>
      {error && <Notice kind="error">{error}</Notice>}
      <div className="reg-list">
        {data.standards.map((s) => {
          const on = data.selected.includes(s.id);
          return (
            <article key={s.id} className={`reg${on ? " on" : ""}`}>
              <div className="reg-head">
                <label className="check-row">
                  <input type="checkbox" checked={on} disabled={!canEdit || busy} onChange={() => toggle(s.id)} aria-label={`Use ${s.name}`} />
                  <span>
                    <strong>{s.name}</strong> <span className="muted small">{s.issuer}</span>
                    <span className="muted small block">{s.summary}</span>
                  </span>
                </label>
                <button className="link-btn small" onClick={() => setOpen(open === s.id ? null : s.id)}>
                  {open === s.id ? "Hide" : `${s.clauses.length} clauses`}
                </button>
              </div>
              {open === s.id && (
                <dl className="clauses">
                  {s.clauses.map((c) => (
                    <div key={`${c.reference}-${c.title}`}>
                      <dt><code>{c.reference}</code> {c.title}</dt>
                      <dd className="muted">{c.summary}</dd>
                    </div>
                  ))}
                </dl>
              )}
            </article>
          );
        })}
      </div>
    </section>
  );
}

// ---------- Templates ----------

interface Template { id: string; step_id: string; name: string; filename: string; chars: number; created_by_name: string; created_at: number }

function Templates({ canEdit }: { canEdit: boolean }) {
  const [data, setData] = useState<{ templates: Template[]; steps: { step_id: string; step: string; document: string }[] } | null>(null);
  const [preview, setPreview] = useState<{ name: string; text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async () => {
    try {
      setData(await api("GET", "/v1/library/templates"));
    } catch (err) {
      setError(messageOf(err));
    }
  }, []);
  useEffect(() => {
    load();
  }, [load]);
  if (!data) return <p className="muted">Loading…</p>;

  return (
    <section className="card">
      <h2>Company templates</h2>
      <p className="muted">
        Give Peter your company's template for a document (Word, PDF, text or Markdown). Drafts then follow its section
        headings, order and table columns. One template per document; a new upload replaces the old one.
      </p>
      {error && <Notice kind="error">{error}</Notice>}
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Document</th>
              <th>Template</th>
              <th aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {data.steps.map((s) => {
              const t = data.templates.find((x) => x.step_id === s.step_id);
              return (
                <tr key={s.step_id}>
                  <td>
                    <strong>{s.document}</strong>
                    <div className="muted small">Step: {s.step}</div>
                  </td>
                  <td>
                    {t ? (
                      <>
                        <button className="link-btn" onClick={() => api<{ name: string; text: string }>("GET", `/v1/library/templates/${t.id}`).then(setPreview)}>
                          {t.name}
                        </button>
                        <div className="muted small">{t.filename} · added by {t.created_by_name}, {formatTime(t.created_at)}</div>
                      </>
                    ) : (
                      <span className="muted">Peter's default structure</span>
                    )}
                  </td>
                  <td className="actions">
                    {canEdit && <TemplateUpload stepId={s.step_id} hasTemplate={!!t} onDone={load} onError={setError} />}
                    {canEdit && t && (
                      <button className="btn ghost danger" onClick={() => api("DELETE", `/v1/library/templates/${t.id}`).then(load).catch((e) => setError(messageOf(e)))}>
                        Remove
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {preview && (
        <div className="zoom" role="dialog" aria-label={preview.name} onClick={() => setPreview(null)}>
          <figure onClick={(e) => e.stopPropagation()} className="template-preview">
            <figcaption>
              <span>{preview.name}: the text Peter reads</span>
              <button className="btn ghost" onClick={() => setPreview(null)}>Close</button>
            </figcaption>
            <pre>{preview.text}</pre>
          </figure>
        </div>
      )}
    </section>
  );
}

function TemplateUpload({ stepId, hasTemplate, onDone, onError }: { stepId: string; hasTemplate: boolean; onDone: () => void; onError: (m: string) => void }) {
  const input = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  return (
    <label className="btn ghost file-btn">
      {busy ? "Uploading…" : hasTemplate ? "Replace" : "Add template"}
      <input
        ref={input}
        type="file"
        accept=".docx,.pdf,.txt,.md"
        disabled={busy}
        onChange={async (e) => {
          const file = e.target.files?.[0];
          if (!file) return;
          setBusy(true);
          try {
            const form = new FormData();
            form.append("step_id", stepId);
            form.append("name", file.name.replace(/\.[^.]+$/, ""));
            form.append("file", file);
            await uploadForm("/v1/library/templates", form);
            onDone();
          } catch (err) {
            onError(messageOf(err));
          } finally {
            setBusy(false);
            if (input.current) input.current.value = "";
          }
        }}
      />
    </label>
  );
}

/** Multipart upload with extra fields (uploadFile only sends the file). */
async function uploadForm(path: string, form: FormData) {
  return api("POST", path, form);
}

// ---------- Branding ----------

interface BrandingData { company_name: string; doc_prefix: string; footer_text: string; logo_type: string | null }

function Branding({ canEdit }: { canEdit: boolean }) {
  const [data, setData] = useState<BrandingData | null>(null);
  const [logoUrl, setLogoUrl] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const { busy, error, run } = useAction();

  const loadLogo = useCallback(async (has: boolean) => {
    setLogoUrl((old) => {
      if (old) URL.revokeObjectURL(old);
      return null;
    });
    if (has) setLogoUrl(await fetchBlobUrl("/v1/workspace/branding/logo").catch(() => null));
  }, []);

  useEffect(() => {
    api<BrandingData>("GET", "/v1/workspace/branding").then((b) => {
      setData(b);
      loadLogo(!!b.logo_type);
    }).catch(() => {});
  }, [loadLogo]);
  if (!data) return <p className="muted">Loading…</p>;

  const set = (k: keyof BrandingData, v: string) => {
    setData({ ...data, [k]: v });
    setSaved(false);
  };
  const example = data.doc_prefix ? `${data.doc_prefix.toUpperCase()}7F3A9C21-URS` : "";

  return (
    <section className="card">
      <h2>Branding on exports</h2>
      <p className="muted">How your company appears on every Word export: logo and name in the page header, a document number on the cover, and footer text on every page.</p>
      <form
        className="stack wide-stack"
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            setData(await api<BrandingData>("PUT", "/v1/workspace/branding", {
              company_name: data.company_name, doc_prefix: data.doc_prefix, footer_text: data.footer_text,
            }));
            setSaved(true);
          });
        }}
      >
        <label className="field">
          <span className="field-label">Company name</span>
          <input value={data.company_name} maxLength={120} disabled={!canEdit} onChange={(e) => set("company_name", e.target.value)} placeholder="Acme Pharma Ltd" />
        </label>
        <label className="field">
          <span className="field-label">Document number prefix</span>
          <input value={data.doc_prefix} maxLength={20} disabled={!canEdit} onChange={(e) => set("doc_prefix", e.target.value)} placeholder="ACME-CSV-" />
          <span className="field-hint">
            Letters, digits, - and _. {example ? <>Documents are numbered like <code>{example}</code> (prefix, session, document).</> : "Leave empty for no document numbers."}
          </span>
        </label>
        <label className="field">
          <span className="field-label">Footer text</span>
          <input value={data.footer_text} maxLength={200} disabled={!canEdit} onChange={(e) => set("footer_text", e.target.value)} placeholder="Confidential" />
        </label>
        {error && <Notice kind="error">{error}</Notice>}
        {saved && <Notice kind="ok">Saved. New exports use it.</Notice>}
        {canEdit && <div><button className="btn primary" disabled={busy}>Save</button></div>}
      </form>

      <div className="logo-row">
        <div className="logo-box">{logoUrl ? <img src={logoUrl} alt="Company logo" /> : <span className="muted small">No logo</span>}</div>
        {canEdit && (
          <div className="row-actions">
            <label className="btn ghost file-btn">
              {data.logo_type ? "Replace logo" : "Add logo"}
              <input
                type="file"
                accept="image/png,image/jpeg"
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (!file) return;
                  run(async () => {
                    const b = await uploadFile<BrandingData>("/v1/workspace/branding/logo", file);
                    setData(b);
                    await loadLogo(true);
                  });
                  e.target.value = "";
                }}
              />
            </label>
            {data.logo_type && (
              <button className="btn ghost danger" onClick={() => run(async () => {
                await api("DELETE", "/v1/workspace/branding/logo");
                setData({ ...data, logo_type: null });
                await loadLogo(false);
              })}>Remove logo</button>
            )}
            <span className="muted small">PNG or JPEG, up to 1 MB. A wide logo works best.</span>
          </div>
        )}
      </div>
    </section>
  );
}
