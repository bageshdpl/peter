import { useCallback, useEffect, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { api, formatTime } from "./api";
import { messageOf, Notice, useAction } from "./ui";

export interface Share {
  id: string;
  session_id: string;
  session_title: string;
  project_name: string | null;
  step_id: string | null;
  step_label: string | null;
  user_id: string;
  user_name: string;
  shared_by: string;
  shared_by_name: string;
  kind: "share" | "review";
  message: string;
  created_at: number;
  done_at: number | null;
}

interface Member {
  user_id: string;
  name: string;
  email: string;
  role: string;
}

export function shareLink(s: { session_id: string; step_id: string | null }) {
  return `/sessions/${s.session_id}${s.step_id ? `?step=${s.step_id}` : ""}`;
}

/** Sharing a session, or one of its documents, with a colleague. */
export function SharePanel(props: {
  sessionId: string;
  meId: string;
  steps: { id: string; label: string }[];
  draftedSteps: string[];
  currentStep: string | null;
  canWrite: boolean;
  isAdmin: boolean;
  onClose: () => void;
}) {
  const { sessionId, meId, steps, draftedSteps, currentStep, canWrite, isAdmin, onClose } = props;
  const [members, setMembers] = useState<Member[]>([]);
  const [shares, setShares] = useState<Share[] | null>(null);
  const [userId, setUserId] = useState("");
  const [stepId, setStepId] = useState(currentStep && draftedSteps.includes(currentStep) ? currentStep : "");
  const [review, setReview] = useState(false);
  const [message, setMessage] = useState("");
  const [sent, setSent] = useState<string | null>(null);
  const { busy, error, setError, run } = useAction();
  const base = `/v1/sessions/${sessionId}/shares`;

  const load = useCallback(async () => {
    try {
      const [m, s] = await Promise.all([api<{ members: Member[] }>("GET", "/v1/members"), api<{ shares: Share[] }>("GET", base)]);
      setMembers(m.members.filter((x) => x.user_id !== meId));
      setShares(s.shares);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [base, meId, setError]);
  useEffect(() => {
    load();
  }, [load]);

  const person = members.find((m) => m.user_id === userId);
  const canReview = !!stepId && draftedSteps.includes(stepId) && person?.role !== "viewer";

  return (
    <section className="card share-panel">
      <div className="card-head">
        <div>
          <h2>Share</h2>
          <p className="muted">
            Everyone in the workspace can open this session. Sharing points someone at it, and they get a notification.
            Ask for a review and they are reminded until they sign.
          </p>
        </div>
        <button className="btn ghost" onClick={onClose}>Close</button>
      </div>

      {canWrite && (
        <form
          className="share-form"
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              await api("POST", base, { user_id: userId, step_id: stepId || null, review: review && canReview, message });
              setSent(`${person?.name ?? "They"} will see it in their notifications.`);
              setMessage("");
              setReview(false);
              await load();
            });
          }}
        >
          <label className="field">
            <span className="field-label">With</span>
            <select value={userId} onChange={(e) => setUserId(e.target.value)} required>
              <option value="" disabled>Choose a person</option>
              {members.map((m) => <option key={m.user_id} value={m.user_id}>{m.name} ({m.email}){m.role === "viewer" ? " · viewer" : ""}</option>)}
            </select>
          </label>
          <label className="field">
            <span className="field-label">What</span>
            <select value={stepId} onChange={(e) => setStepId(e.target.value)}>
              <option value="">The whole session</option>
              {steps.map((s) => (
                <option key={s.id} value={s.id}>{s.label}{draftedSteps.includes(s.id) ? "" : " (not drafted yet)"}</option>
              ))}
            </select>
          </label>
          <label className="field share-message">
            <span className="field-label">Message (optional)</span>
            <input value={message} onChange={(e) => setMessage(e.target.value)} maxLength={1000} placeholder="Please check the risk ratings" />
          </label>
          <label className={`check-row${canReview ? "" : " disabled"}`}>
            <input type="checkbox" checked={review && canReview} disabled={!canReview} onChange={(e) => setReview(e.target.checked)} />
            <span>
              Ask them to review and sign this document
              {!canReview && (
                <span className="small muted"> — {!stepId ? "choose a drafted document" : !draftedSteps.includes(stepId) ? "draft it first" : "viewers cannot sign"}</span>
              )}
            </span>
          </label>
          <div className="row-actions">
            <button className="btn primary" disabled={busy || !userId}>{review && canReview ? "Ask for review" : "Share"}</button>
          </div>
        </form>
      )}
      {error && <Notice kind="error">{error}</Notice>}
      {sent && <Notice kind="ok">{sent}</Notice>}

      {shares && shares.length > 0 && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>With</th>
                <th>What</th>
                <th className="hide-sm">By</th>
                <th>Status</th>
                <th aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {shares.map((s) => (
                <tr key={s.id}>
                  <td>{s.user_name}</td>
                  <td className="small">
                    {s.step_label ?? "Whole session"}
                    {s.message && <div className="muted">“{s.message}”</div>}
                  </td>
                  <td className="hide-sm small muted">{s.shared_by_name}<div>{formatTime(s.created_at)}</div></td>
                  <td>
                    <span className={`status-pill ${s.done_at ? "passed" : s.kind === "review" ? "not_tested" : ""}`}>
                      {s.kind === "review" ? (s.done_at ? "Signed" : "Review asked") : s.done_at ? "Done" : "Shared"}
                    </span>
                  </td>
                  <td>
                    {(s.shared_by === meId || isAdmin) && (
                      <button className="btn ghost danger" disabled={busy}
                        onClick={() => run(async () => { await api("DELETE", `/v1/shares/${s.id}`); await load(); })}>
                        Remove
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

/** What colleagues have shared with me, for the home page. */
export function SharedWithMe() {
  const [shares, setShares] = useState<Share[]>([]);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async () => {
    try {
      setShares((await api<{ shares: Share[] }>("GET", "/v1/shared-with-me")).shares);
    } catch (err) {
      setError(messageOf(err));
    }
  }, []);
  useEffect(() => {
    load();
  }, [load]);
  const open = shares.filter((s) => !s.done_at);
  if (!error && open.length === 0) return null;

  return (
    <section className="card">
      <h2>Shared with you</h2>
      {error && <Notice kind="error">{error}</Notice>}
      <ul className="share-list">
        {open.map((s) => (
          <li key={s.id}>
            <div>
              <Link to={shareLink(s)}>
                {s.step_label ? `${s.step_label} · ` : ""}{s.session_title}
              </Link>
              {s.kind === "review" && <span className="status-pill not_tested">Review and sign</span>}
              <div className="small muted">
                From {s.shared_by_name}, {formatTime(s.created_at)}{s.project_name ? ` · ${s.project_name}` : ""}
                {s.message && <> · “{s.message}”</>}
              </div>
            </div>
            <button className="btn ghost" onClick={() => api("POST", `/v1/shares/${s.id}/done`, { done: true }).then(load).catch((e) => setError(messageOf(e)))}>
              Mark done
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** The number of unread notifications, re-checked every minute and on every page change. */
export function useUnread(workspaceId: string) {
  const [unread, setUnread] = useState(0);
  const location = useLocation();
  const refresh = useCallback(() => {
    api<{ unread: number }>("GET", "/v1/notifications/unread").then((r) => setUnread(r.unread)).catch(() => {});
  }, []);
  useEffect(() => {
    refresh();
  }, [refresh, location.pathname, location.search, workspaceId]);
  useEffect(() => {
    const t = setInterval(refresh, 60_000);
    const onRead = () => refresh();
    window.addEventListener("peter:notifications", onRead);
    return () => {
      clearInterval(t);
      window.removeEventListener("peter:notifications", onRead);
    };
  }, [refresh]);
  return unread;
}
