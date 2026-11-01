import { useState } from "react";
import { ApiError } from "./api";

export function Field(props: {
  label: string;
  type?: string;
  value: string;
  onChange: (v: string) => void;
  autoComplete?: string;
  required?: boolean;
  minLength?: number;
  hint?: string;
  autoFocus?: boolean;
}) {
  return (
    <label className="field">
      <span className="field-label">{props.label}</span>
      <input
        type={props.type ?? "text"}
        value={props.value}
        onChange={(e) => props.onChange(e.target.value)}
        autoComplete={props.autoComplete}
        required={props.required ?? true}
        minLength={props.minLength}
        autoFocus={props.autoFocus}
      />
      {props.hint && <span className="field-hint">{props.hint}</span>}
    </label>
  );
}

export function Notice({ kind, children }: { kind: "error" | "ok"; children: React.ReactNode }) {
  return (
    <div className={`notice ${kind}`} role={kind === "error" ? "alert" : "status"}>
      {children}
    </div>
  );
}

export function messageOf(err: unknown) {
  if (err instanceof ApiError) return err.message;
  return "Could not reach the Peter API. Is the server running?";
}

/** Runs an async action, tracking whether it is busy and its error message. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function run(fn: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setBusy(false);
    }
  }
  return { busy, error, setError, run };
}

export function AuthCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="auth-page">
      <div className="auth-card">
        <div className="brand">
          <span className="brand-mark" aria-hidden>P</span>
          <span>Peter</span>
        </div>
        <h1>{title}</h1>
        {children}
      </div>
    </div>
  );
}

export const PASSWORD_HINT = "At least 10 characters.";
