import { useEffect, useState } from "react";
import { Navigate, Route, Routes, useLocation } from "react-router-dom";
import { getSession, onSessionChange, type Session } from "./api";
import Shell from "./Shell";
import { AcceptInvite, ForgotPassword, ResetPassword, SignIn, SignUp } from "./pages/Auth";
import Home from "./pages/Home";
import ProjectPage from "./pages/ProjectPage";
import SessionPage from "./pages/SessionPage";
import Members from "./pages/Members";
import AuditLog from "./pages/AuditLog";
import Settings from "./pages/Settings";

export function useSession() {
  const [session, setSession] = useState<Session | null>(getSession());
  useEffect(() => {
    const off = onSessionChange(setSession);
    return () => {
      off();
    };
  }, []);
  return session;
}

function RequireSession({ children }: { children: React.ReactNode }) {
  const session = useSession();
  const location = useLocation();
  if (!session) return <Navigate to="/signin" replace state={{ from: location.pathname }} />;
  return <>{children}</>;
}

export default function App() {
  return (
    <Routes>
      <Route path="/signin" element={<SignIn />} />
      <Route path="/signup" element={<SignUp />} />
      <Route path="/forgot" element={<ForgotPassword />} />
      <Route path="/reset" element={<ResetPassword />} />
      <Route path="/invite" element={<AcceptInvite />} />
      <Route
        element={
          <RequireSession>
            <Shell />
          </RequireSession>
        }
      >
        <Route index element={<Home />} />
        <Route path="projects/:id" element={<ProjectPage />} />
        <Route path="sessions/:id" element={<SessionPage />} />
        <Route path="members" element={<Members />} />
        <Route path="audit" element={<AuditLog />} />
        <Route path="settings" element={<Settings />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
