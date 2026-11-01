import { useCallback, useEffect, useState } from "react";
import { api, formatTime, type AuditEvent } from "../api";
import { useSession } from "../App";
import { messageOf, Notice } from "../ui";

const ACTIONS: Record<string, string> = {
  "workspace.created": "Created the workspace",
  "workspace.renamed": "Renamed the workspace",
  "auth.login": "Signed in",
  "member.invited": "Invited a person",
  "invite.revoked": "Withdrew an invite",
  "member.joined": "Joined the workspace",
  "member.role_changed": "Changed a role",
  "member.removed": "Removed a member",
  "member.left": "Left the workspace",
  "user.password_changed": "Changed their password",
  "project.created": "Created a project",
  "project.updated": "Edited a project",
  "project.archived": "Archived a project",
  "project.restored": "Restored a project",
  "session.created": "Started a session",
  "session.updated": "Edited a session",
  "session.archived": "Archived a session",
  "session.restored": "Restored a session",
  "source.uploaded": "Added a file",
  "source.removed": "Removed a file",
  "user.mfa_enabled": "Turned on two-factor sign-in",
  "user.mfa_disabled": "Turned off two-factor sign-in",
  "user.recovery_codes_renewed": "Made new recovery codes",
  "auth.session_revoked": "Signed out a device",
  "auth.other_sessions_revoked": "Signed out all other devices",
  "workspace.mfa_policy_changed": "Changed the two-factor requirement",
  "document.drafted": "Drafted a document",
  "document.draft_failed": "A draft failed",
  "document.edited": "Edited a document",
  "document.generated": "Built the traceability matrix",
  "document.signed": "Signed a document",
  "document.exported": "Exported",
  "comment.added": "Commented",
  "comment.replied": "Replied to a comment",
  "comment.resolved": "Resolved a comment",
  "comment.reopened": "Reopened a comment",
  "step.status_changed": "Changed a step's status",
  "observation.removed": "Removed an observed step",
  "extension.connected": "Connected the extension",
  "library.standards_changed": "Changed the regulations in scope",
  "library.template_added": "Added a company template",
  "library.template_removed": "Removed a company template",
  "library.branding_changed": "Changed the branding",
  "library.logo_changed": "Changed the logo",
};

/** "name: Old → New; vendor: … " for update events. */
function changes(d: Record<string, unknown>) {
  return Object.entries(d)
    .filter(([, v]) => v && typeof v === "object" && "to" in (v as object))
    .map(([k, v]) => {
      const c = v as { from: unknown; to: unknown };
      return `${k.replace("_", " ")}: ${String(c.from ?? "none") || "empty"} → ${String(c.to ?? "none") || "empty"}`;
    })
    .join("; ");
}

function describe(e: AuditEvent): string {
  const d = e.detail ?? {};
  switch (e.action) {
    case "member.invited":
      return `${e.target} as ${String(d.role)}`;
    case "invite.revoked":
      return e.target ?? "";
    case "member.role_changed":
      return `${String(d.from)} → ${String(d.to)}`;
    case "workspace.created":
    case "workspace.renamed":
      return String(d.name ?? "");
    case "member.joined":
      return `as ${String(d.role)}`;
    case "auth.login":
      return d.mfa ? (d.method === "recovery_code" ? "with a recovery code" : "with two-factor") : "";
    case "workspace.mfa_policy_changed":
      return d.require_mfa ? "required" : "optional";
    case "document.signed":
      return `${String(d.meaning)}, version ${String(d.version)}`;
    case "library.template_added":
      return String(d.name ?? "");
    case "library.standards_changed":
      return Array.isArray(d.selected) ? (d.selected as string[]).join(", ") || "none" : "";
    case "document.exported":
      return String(d.format ?? "").toUpperCase();
    case "project.created":
      return String(d.name ?? "");
    case "session.created":
      return String(d.title ?? "");
    case "source.uploaded":
    case "source.removed":
      return String(d.filename ?? "");
    case "project.updated":
    case "session.updated":
      return changes(d);
    default:
      return "";
  }
}

interface Page {
  events: AuditEvent[];
  next_before: number | null;
}

export default function AuditLog() {
  const session = useSession()!;
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [next, setNext] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchPage = useCallback(async (before: number | null) => {
    setLoading(true);
    try {
      const page = await api<Page>("GET", `/v1/audit?limit=50${before ? `&before=${before}` : ""}`);
      setEvents((prev) => (before ? [...prev, ...page.events] : page.events));
      setNext(page.next_before);
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchPage(null);
  }, [fetchPage, session.workspace.id]);

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.workspace.name}</p>
          <h1>Audit trail</h1>
          <p className="muted">Every change, newest first. Entries cannot be edited or deleted.</p>
        </div>
      </header>

      <section className="card">
        {error && <Notice kind="error">{error}</Notice>}
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>When</th>
                <th>Who</th>
                <th>What</th>
                <th>Details</th>
              </tr>
            </thead>
            <tbody>
              {events.map((e) => (
                <tr key={e.id}>
                  <td className="muted nowrap">{formatTime(e.at)}</td>
                  <td>{e.actor_email ?? "—"}</td>
                  <td>{ACTIONS[e.action] ?? e.action}</td>
                  <td className="muted">{describe(e)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {!loading && events.length === 0 && <p className="muted">Nothing recorded yet.</p>}
        {next && (
          <button className="btn ghost" disabled={loading} onClick={() => fetchPage(next)}>
            Load older
          </button>
        )}
      </section>
    </div>
  );
}
