import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

type NotesEvent = { kind: "mom"; data: string } | { kind: "pending"; data: string };

/** The minutes of the current meeting, and why an update is pending when one failed. */
export function useMom(meetingId: number) {
  const [mom, setMom] = useState("");
  const [pending, setPending] = useState<string | null>(null);

  useEffect(() => {
    setMom("");
    setPending(null);
  }, [meetingId]);

  useEffect(() => {
    const unlisten = listen<NotesEvent>("notes-event", ({ payload }) => {
      if (payload.kind === "mom") {
        setMom(payload.data);
        setPending(null);
      } else {
        setPending(payload.data);
      }
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  return { mom, pending };
}

export function rebuildMom(): Promise<void> {
  return invoke("rebuild_mom");
}

export function setUserNotes(text: string): Promise<void> {
  return invoke("set_user_notes", { text });
}

export function ask(question: string): Promise<string> {
  return invoke("ask", { question });
}
