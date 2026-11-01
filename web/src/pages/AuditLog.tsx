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
};

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
