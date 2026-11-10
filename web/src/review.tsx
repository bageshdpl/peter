import { useCallback, useEffect, useState } from "react";
import { api, downloadFile, formatTime, type Comment, type Signature } from "./api";
import type { SessionEvents } from "./events";
import { useLocation } from "react-router-dom";
import { useSession } from "./App";
import { confirmSignatureWithSso } from "./pages/Sso";
import { messageOf, Notice, useAction } from "./ui";

// ---------- Export ----------

export function ExportButtons(props: { base: string; title: string; version: number; hasTables: boolean }) {
  const { base, title, version, hasTables } = props;
  const [error, setError] = useState<string | null>(null);
  const get = (format: "docx" | "pdf" | "xlsx", name: string) =>
    downloadFile(`${base}/export/${format}?version=${version}`, name).catch((e) => setError(messageOf(e)));
  return (
    <div className="export-buttons">
      <button className="btn ghost" onClick={() => get("docx", `${title} v${version}.docx`)}>Word</button>
      <button className="btn ghost" onClick={() => get("pdf", `${title} v${version}.pdf`)}>PDF</button>
      {hasTables && (
        <button className="btn ghost" onClick={() => get("xlsx", `${title} v${version} tables.xlsx`)}>Excel</button>
      )}
      {error && <span className="bad small">{error}</span>}
    </div>
  );
}

// ---------- Signatures ----------

const MEANING: Record<Signature["meaning"], string> = { reviewed: "Reviewed", approved: "Approved" };

export function SignaturePanel(props: {
  base: string;
  title: string;
  version: number;
  signatures: Signature[];
  canSign: boolean;
  onSigned: () => void;
}) {
  const { base, title, version, signatures, canSign, onSigned } = props;
  const [signing, setSigning] = useState(false);
  const [meaning, setMeaning] = useState<Signature["meaning"]>("reviewed");
  const [password, setPassword] = useState("");
  const { busy, error, run } = useAction();
  // People who came in through the identity provider confirm there; they may have no Peter password.
  const viaSso = useSession()?.sso === true;
  const location = useLocation();
  const here = signatures.filter((s) => s.version === version);
  const older = signatures.filter((s) => s.version !== version);

  return (
    <div className="signatures">
      <div className="row-head">
        <h3>Signatures on version {version}</h3>
        {canSign && !signing && <button className="btn ghost" onClick={() => setSigning(true)}>Sign this version</button>}
      </div>
      {here.length === 0 ? (
        <p className="muted small">Not signed yet.</p>
      ) : (
        <ul className="sig-list">
          {here.map((s) => (
            <li key={`${s.signer_email}-${s.meaning}`}>
              <span className={`sig-badge ${s.meaning}`}>{MEANING[s.meaning]}</span>
              <span><strong>{s.signer_name}</strong> <span className="muted">({s.signer_email})</span></span>
              <span className="muted small">{formatTime(s.signed_at)}</span>
            </li>
          ))}
        </ul>
      )}
      {older.length > 0 && (
        <p className="muted small">
          Earlier versions were signed {older.length} {older.length === 1 ? "time" : "times"}. A new version needs signing again.
        </p>
      )}

      {signing && (
        <form
          className="sign-form"
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              if (viaSso) {
                await confirmSignatureWithSso({ base, version, meaning, returnTo: location.pathname + location.search });
                return;
              }
              await api("POST", `${base}/signatures`, { version, meaning, password });
              setPassword("");
              setSigning(false);
              onSigned();
            });
          }}
        >
          <fieldset className="meaning">
            <legend className="field-label">Meaning of your signature</legend>
            {(["reviewed", "approved"] as const).map((m) => (
              <label key={m}>
                <input type="radio" name="meaning" checked={meaning === m} onChange={() => setMeaning(m)} /> {MEANING[m]}
              </label>
            ))}
          </fieldset>
          <p className="statement">
            By signing, I confirm that I have {meaning === "approved" ? "reviewed and approve" : "reviewed"} version {version}{" "}
            of “{title}”. My name, this meaning, and the date and time are recorded with the version.
          </p>
          {viaSso ? (
            <p className="muted small">
              To sign, you will sign in again at your company's identity provider. Peter brings you back here and records
              the signature.
            </p>
          ) : (
            <label className="field">
              <span className="field-label">Enter your password to sign</span>
              <input type="password" required autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} autoFocus />
            </label>
          )}
          {error && <Notice kind="error">{error}</Notice>}
          <div className="form-actions">
            <button type="button" className="btn ghost" onClick={() => setSigning(false)}>Cancel</button>
            <button className="btn primary" disabled={busy}>
              {viaSso ? `Confirm with SSO and sign as ${MEANING[meaning].toLowerCase()}` : `Sign as ${MEANING[meaning].toLowerCase()}`}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}

// ---------- Comments ----------

export function CommentsPanel(props: {
  base: string;
  stepId: string;
  userId: string;
  canResolveAny: boolean;
  canComment: boolean;
  quote: string;
  clearQuote: () => void;
  events: SessionEvents;
}) {
  const { base, stepId, userId, canResolveAny, canComment, quote, clearQuote, events } = props;
  const [comments, setComments] = useState<Comment[] | null>(null);
  const [text, setText] = useState("");
  const [showResolved, setShowResolved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { busy, run } = useAction();

  const load = useCallback(async () => {
    try {
      setComments((await api<{ comments: Comment[] }>("GET", `${base}/comments`)).comments);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [base]);

  useEffect(() => {
    load();
  }, [load]);
  const { subscribe } = events;
  useEffect(
    () => subscribe((name, data) => {
      if (name === "open" || (name === "comment" && (data as { step_id?: string })?.step_id === stepId)) load();
    }),
    [subscribe, load, stepId],
  );

  async function act(fn: () => Promise<unknown>) {
    setError(null);
    try {
      await fn();
      await load();
    } catch (err) {
      setError(messageOf(err));
    }
  }

  const threads = (comments ?? []).filter((c) => !c.parent_id);
  const replies = (id: string) => (comments ?? []).filter((c) => c.parent_id === id);
  const open = threads.filter((t) => !t.resolved_at);
  const resolved = threads.filter((t) => t.resolved_at);

  return (
    <div className="comments">
      <div className="row-head">
        <h3>Comments {open.length > 0 && <span className="count">{open.length} open</span>}</h3>
        {resolved.length > 0 && (
          <button className="link-btn small" onClick={() => setShowResolved((v) => !v)}>
            {showResolved ? "Hide" : "Show"} {resolved.length} resolved
          </button>
        )}
      </div>
      {error && <Notice kind="error">{error}</Notice>}

      {canComment && (
        <form
          className="comment-form"
          onSubmit={(e) => {
            e.preventDefault();
            run(async () => {
              await api("POST", `${base}/comments`, { body: text, quote });
              setText("");
              clearQuote();
              await load();
            });
          }}
        >
          {quote ? (
            <div className="quote-chip">
              <span>On: “{quote.length > 140 ? `${quote.slice(0, 140)}…` : quote}”</span>
              <button type="button" className="link-btn small" onClick={clearQuote}>Clear</button>
            </div>
          ) : (
            <p className="muted small">Select text in the document to comment on it, or comment on the whole document.</p>
          )}
          <textarea rows={2} maxLength={4000} required value={text} onChange={(e) => setText(e.target.value)}
            placeholder="Add a comment for the team. Type @ and a name to notify someone." aria-label="New comment" />
          <div className="form-actions">
            <button className="btn primary" disabled={busy || !text.trim()}>Comment</button>
          </div>
        </form>
      )}

      {comments && threads.length === 0 && <p className="muted small">No comments yet.</p>}
      {[...open, ...(showResolved ? resolved : [])].map((t) => (
        <Thread
          key={t.id}
          thread={t}
          replies={replies(t.id)}
          canResolve={canResolveAny || t.author_id === userId}
          canReply={canComment}
          onResolve={() => act(() => api("POST", `/v1/comments/${t.id}/${t.resolved_at ? "reopen" : "resolve"}`))}
          onReply={(body) => act(() => api("POST", `${base}/comments`, { body, parent_id: t.id }))}
        />
      ))}
    </div>
  );
}

function Thread(props: {
  thread: Comment;
  replies: Comment[];
  canResolve: boolean;
  canReply: boolean;
  onResolve: () => void;
  onReply: (body: string) => Promise<void>;
}) {
  const { thread, replies, canResolve, canReply, onResolve, onReply } = props;
  const [reply, setReply] = useState("");
  const [replying, setReplying] = useState(false);
  return (
    <article className={`thread${thread.resolved_at ? " resolved" : ""}`}>
      {thread.quote && <blockquote>{thread.quote}</blockquote>}
      {[thread, ...replies].map((c) => (
        <div key={c.id} className="comment">
          <div className="comment-meta">
            <strong>{c.author_name}</strong>
            <span className="muted small">{formatTime(c.created_at)} · on v{c.version}</span>
          </div>
          <p>{c.body}</p>
        </div>
      ))}
      {thread.resolved_at && <p className="muted small">Resolved by {thread.resolved_by_name}, {formatTime(thread.resolved_at)}</p>}
      <div className="thread-actions">
        {canReply && !thread.resolved_at && !replying && <button className="link-btn small" onClick={() => setReplying(true)}>Reply</button>}
        {canResolve && <button className="link-btn small" onClick={onResolve}>{thread.resolved_at ? "Reopen" : "Resolve"}</button>}
      </div>
      {replying && (
        <form
          className="reply-form"
          onSubmit={async (e) => {
            e.preventDefault();
            await onReply(reply);
            setReply("");
            setReplying(false);
          }}
        >
          <textarea rows={2} maxLength={4000} required value={reply} onChange={(e) => setReply(e.target.value)} aria-label="Reply" autoFocus />
          <div className="form-actions">
            <button type="button" className="btn ghost" onClick={() => setReplying(false)}>Cancel</button>
            <button className="btn primary" disabled={!reply.trim()}>Reply</button>
          </div>
        </form>
      )}
    </article>
  );
}
