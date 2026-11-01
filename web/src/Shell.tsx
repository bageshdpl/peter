import { useEffect, useState } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { atLeast, logout, reloadSession, startSession } from "./api";
import { useSession } from "./App";

export default function Shell() {
  const session = useSession()!;
  const navigate = useNavigate();
  const location = useLocation();
  const [switching, setSwitching] = useState(false);
  const isAdmin = atLeast(session.workspace.role, "admin");

  // Roles and workspaces can change on another device, so re-read them on
  // every page change rather than trusting what was saved at sign-in.
  useEffect(() => {
    reloadSession().catch(() => {});
  }, [location.pathname]);

  async function switchTo(workspaceId: string) {
    if (workspaceId === session.workspace.id) return;
    setSwitching(true);
    try {
      await startSession("/v1/auth/switch", { workspace_id: workspaceId });
      navigate("/");
    } finally {
      setSwitching(false);
    }
  }

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden>P</span>
          <span>Peter</span>
        </div>

        <label className="field compact">
          <span className="field-label">Workspace</span>
          <select
            value={session.workspace.id}
            disabled={switching}
            onChange={(e) => switchTo(e.target.value)}
            aria-label="Switch workspace"
          >
            {session.workspaces.map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
              </option>
            ))}
          </select>
        </label>

        <nav className="nav">
          <NavLink to="/" end>Projects</NavLink>
          <NavLink to="/members">Members</NavLink>
          {isAdmin && <NavLink to="/audit">Audit trail</NavLink>}
          <NavLink to="/settings">Settings</NavLink>
        </nav>

        <div className="sidebar-foot">
          <div className="who">
            <strong>{session.user.name}</strong>
            <span className="muted">{session.user.email}</span>
            <span className={`role role-${session.workspace.role}`}>{session.workspace.role}</span>
          </div>
          <button className="btn ghost" onClick={() => logout().then(() => navigate("/signin"))}>
            Sign out
          </button>
        </div>
      </aside>
      <main className="main">
        <Outlet />
      </main>
    </div>
  );
}
