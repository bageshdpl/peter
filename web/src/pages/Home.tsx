import { Link } from "react-router-dom";
import { atLeast } from "../api";
import { useSession } from "../App";

export default function Home() {
  const session = useSession()!;
  const isAdmin = atLeast(session.workspace.role, "admin");

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <p className="eyebrow">{session.workspace.name}</p>
          <h1>Welcome, {session.user.name.split(" ")[0]}</h1>
        </div>
      </header>

      <section className="card empty">
        <h2>Validation projects</h2>
        <p className="muted">
          Projects are where you validate a system: you observe it running, and Peter drafts the documents from what it
          saw. They arrive in the next milestone.
        </p>
      </section>

      <section className="grid-2">
        <Link to="/members" className="card link-card">
          <h2>Members</h2>
          <p className="muted">
            {isAdmin ? "Invite people and set what they can do." : "See who works in this workspace."}
          </p>
        </Link>
        {isAdmin ? (
          <Link to="/audit" className="card link-card">
            <h2>Audit trail</h2>
            <p className="muted">Every change in the workspace, who made it and when.</p>
          </Link>
        ) : (
          <Link to="/settings" className="card link-card">
            <h2>Settings</h2>
            <p className="muted">Your password and workspaces.</p>
          </Link>
        )}
      </section>
    </div>
  );
}
