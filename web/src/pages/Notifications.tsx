import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, formatTime } from "../api";
import { useSession } from "../App";
import { shareLink } from "../sharing";
import { messageOf, Notice } from "../ui";

interface Item {
  id: string;
  kind: string;
  session_id: string | null;
  step_id: string | null;
  text: string;
  actor_name: string | null;
  created_at: number;
  read_at: number | null;
}

const KIND: Record<string, string> = { share: "Shared", review: "Review", mention: "Mention", reply: "Reply", signed: "Signed" };

export default function Notifications() {
  const session = useSession()!;
  const navigate = useNavigate();
  const [items, setItems] = useState<Item[] | null>(null);
  const [onlyUnread, setOnlyUnread] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setItems((await api<{ notifications: Item[] }>("GET", `/v1/notifications?unread=${onlyUnread}`)).notifications);
      setError(null);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [onlyUnread]);
  useEffect(() => {
    load();
  }, [load, session.workspace.id]);

  async function markRead(body: { ids?: string[]; all?: boolean }) {
    await api("POST", "/v1/notifications/read", body);
    window.dispatchEvent(new Event("peter:notifications"));
  }

  async function open(n: Item) {
    if (!n.read_at) await markRead({ ids: [n.id] }).catch(() => {});
    if (n.session_id) navigate(shareLink({ session_id: n.session_id, step_id: n.step_id }));
    else load();
  }

  const unread = items?.filter((n) => !n.read_at).length ?? 0;
  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.workspace.name}</p>
          <h1>Notifications</h1>
          <p className="muted">Shares, review requests, replies and @mentions in this workspace.</p>
        </div>
        <div className="head-actions">
          <button className="btn ghost" disabled={!unread} onClick={() => markRead({ all: true }).then(load).catch((e) => setError(messageOf(e)))}>
            Mark all read
          </button>
        </div>
      </header>
      {error && <Notice kind="error">{error}</Notice>}
      <section className="card">
        <div className="tabs" role="tablist">
          <button role="tab" aria-selected={!onlyUnread} onClick={() => setOnlyUnread(false)}>All</button>
          <button role="tab" aria-selected={onlyUnread} onClick={() => setOnlyUnread(true)}>Unread</button>
        </div>
        {items && items.length === 0 && <p className="muted">{onlyUnread ? "Nothing unread." : "No notifications yet."}</p>}
        <ul className="notification-list">
          {items?.map((n) => (
            <li key={n.id} className={n.read_at ? "" : "unread"}>
              <button className="notification" onClick={() => open(n)}>
                <span className="kind">{KIND[n.kind] ?? n.kind}</span>
                <span className="text">{n.text}</span>
                <span className="small muted">{formatTime(n.created_at)}</span>
              </button>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
