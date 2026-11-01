import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, atLeast, formatTime, reloadSession, ROLES, setSession, type Invite, type Member, type Role } from "../api";
import { useSession } from "../App";
import { messageOf, Notice, useAction } from "../ui";

const ROLE_HELP: Record<Role, string> = {
  owner: "Everything, including managing owners.",
  admin: "Manage members and see the audit trail.",
  member: "Do validation work.",
  viewer: "Read only.",
};

export default function Members() {
  const session = useSession()!;
  const navigate = useNavigate();
  const myRole = session.workspace.role;
  const isAdmin = atLeast(myRole, "admin");
  const isOwner = myRole === "owner";

  const [members, setMembers] = useState<Member[] | null>(null);
  const [invites, setInvites] = useState<Invite[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rowError, setRowError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const data = await api<{ members: Member[]; invites: Invite[] }>("GET", "/v1/members");
      setMembers(data.members);
      setInvites(data.invites);
      setLoadError(null);
    } catch (err) {
      setLoadError(messageOf(err));
    }
  }, []);

  useEffect(() => {
    load();
  }, [load, session.workspace.id]);

  async function act(fn: () => Promise<unknown>) {
    setRowError(null);
    try {
      await fn();
      await load();
    } catch (err) {
      setRowError(messageOf(err));
    }
  }

  async function changeRole(m: Member, role: Role) {
    await act(async () => {
      await api("PATCH", `/v1/members/${m.user_id}`, { role });
      if (m.user_id === session.user.id) await reloadSession();
    });
  }

  async function remove(m: Member) {
    const self = m.user_id === session.user.id;
    const question = self
      ? `Leave ${session.workspace.name}? You will need a new invite to come back.`
      : `Remove ${m.name} (${m.email}) from ${session.workspace.name}?`;
    if (!confirm(question)) return;
    setRowError(null);
    try {
      await api("DELETE", `/v1/members/${m.user_id}`);
      if (self) {
        setSession(null);
        navigate("/signin");
        return;
      }
      await load();
    } catch (err) {
      setRowError(messageOf(err));
    }
  }

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.workspace.name}</p>
          <h1>Members</h1>
        </div>
      </header>

      {isAdmin && <InviteForm canInviteOwner={isOwner} onInvited={load} />}

      <section className="card">
        <h2>People</h2>
        {loadError && <Notice kind="error">{loadError}</Notice>}
        {rowError && <Notice kind="error">{rowError}</Notice>}
        {!members ? (
          <p className="muted">Loading…</p>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Role</th>
                  <th className="hide-sm">Joined</th>
                  <th aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {members.map((m) => {
                  const self = m.user_id === session.user.id;
                  // Only owners can change owners or make new ones.
                  const canEdit = isAdmin && (isOwner || m.role !== "owner");
                  return (
                    <tr key={m.user_id}>
                      <td>
                        <div className="person">
                          <strong>{m.name}{self && <span className="muted"> (you)</span>}</strong>
                          <span className="muted">{m.email}</span>
                        </div>
                      </td>
                      <td>
                        {canEdit ? (
                          <select
                            value={m.role}
                            aria-label={`Role for ${m.name}`}
                            onChange={(e) => changeRole(m, e.target.value as Role)}
                          >
                            {ROLES.filter((r) => isOwner || r !== "owner").map((r) => (
                              <option key={r} value={r}>{r}</option>
                            ))}
                          </select>
                        ) : (
                          <span className={`role role-${m.role}`}>{m.role}</span>
                        )}
                      </td>
                      <td className="muted hide-sm">{formatTime(m.joined_at)}</td>
                      <td className="actions">
                        {(self || canEdit) && (
                          <button className="btn ghost danger" onClick={() => remove(m)}>
                            {self ? "Leave" : "Remove"}
                          </button>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {isAdmin && invites.length > 0 && (
        <section className="card">
          <h2>Open invites</h2>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Email</th>
                  <th>Role</th>
                  <th>Expires</th>
                  <th aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {invites.map((i) => (
                  <tr key={i.id}>
                    <td>{i.email}</td>
                    <td><span className={`role role-${i.role}`}>{i.role}</span></td>
                    <td className="muted">{formatTime(i.expires_at)}</td>
                    <td className="actions">
                      {(isOwner || i.role !== "owner") && (
                        <button className="btn ghost danger" onClick={() => act(() => api("DELETE", `/v1/invites/${i.id}`))}>
                          Withdraw
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      <section className="card">
        <h2>Roles</h2>
        <dl className="role-help">
          {ROLES.map((r) => (
            <div key={r}>
              <dt><span className={`role role-${r}`}>{r}</span></dt>
              <dd className="muted">{ROLE_HELP[r]}</dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  );
}

function InviteForm({ canInviteOwner, onInvited }: { canInviteOwner: boolean; onInvited: () => void }) {
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<Role>("member");
  const [link, setLink] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const { busy, error, run } = useAction();

  return (
    <section className="card">
      <h2>Invite someone</h2>
      <form
        className="inline-form"
        onSubmit={(e) => {
          e.preventDefault();
          run(async () => {
            const res = await api<{ invite_link: string }>("POST", "/v1/members", { email, role });
            setLink(res.invite_link);
            setCopied(false);
            setEmail("");
            onInvited();
          });
        }}
      >
        <label className="field grow">
          <span className="field-label">Email</span>
          <input type="email" required value={email} onChange={(e) => setEmail(e.target.value)} />
        </label>
        <label className="field">
          <span className="field-label">Role</span>
          <select value={role} onChange={(e) => setRole(e.target.value as Role)}>
            {ROLES.filter((r) => canInviteOwner || r !== "owner").map((r) => (
              <option key={r} value={r}>{r}</option>
            ))}
          </select>
        </label>
        <button className="btn primary" disabled={busy}>Send invite</button>
      </form>
      {error && <Notice kind="error">{error}</Notice>}
      {link && (
        <Notice kind="ok">
          <p>Invite created. Peter has no mail server locally, so send this link yourself. It works once.</p>
          <div className="copy-row">
            <code>{link}</code>
            <button
              className="btn ghost"
              type="button"
              onClick={() => navigator.clipboard?.writeText(link).then(() => setCopied(true))}
            >
              {copied ? "Copied" : "Copy"}
            </button>
          </div>
        </Notice>
      )}
    </section>
  );
}
