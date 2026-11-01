import { useCallback, useEffect, useRef, useState } from "react";
import { followEvents } from "./api";

export type SessionListener = (name: string, data: unknown) => void;

/**
 * One live connection per session page, shared by every panel on it.
 * Listeners also receive a synthetic "open" event on each (re)connection, so
 * they can reload whatever they might have missed while it was down.
 */
export function useSessionEvents(sessionId: string) {
  const listeners = useRef(new Set<SessionListener>());
  const [live, setLive] = useState(false);

  useEffect(() => {
    const abort = new AbortController();
    const emit = (name: string, data: unknown) => listeners.current.forEach((fn) => fn(name, data));
    followEvents(
      `/v1/sessions/${sessionId}/events`,
      {
        onOpen: () => {
          setLive(true);
          emit("open", null);
        },
        onEvent: emit,
      },
      abort.signal,
    ).finally(() => setLive(false));
    return () => abort.abort();
  }, [sessionId]);

  const subscribe = useCallback((fn: SessionListener) => {
    listeners.current.add(fn);
    return () => {
      listeners.current.delete(fn);
    };
  }, []);

  return { live, subscribe };
}

export type SessionEvents = ReturnType<typeof useSessionEvents>;
