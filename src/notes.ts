import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

/** The "transcript" kind of this event is handled by `useRecording`. */
type NotesEvent = { kind: "mom"; data: string } | { kind: "pending"; data: string } | { kind: "transcript" };

/** The minutes of the current meeting, and why an update is pending when one failed. */
export function useMom(meetingId: number, savedMom: string) {
  const [mom, setMom] = useState("");
  const [pending, setPending] = useState<string | null>(null);

  useEffect(() => {
    setMom(savedMom);
    setPending(null);
    // Only a change of meeting resets the minutes; `savedMom` belongs to that meeting.
  }, [meetingId]);

  useEffect(() => {
    const unlisten = listen<NotesEvent>("notes-event", ({ payload }) => {
      if (payload.kind === "mom") {
        setMom(payload.data);
        setPending(null);
      } else if (payload.kind === "pending") {
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
