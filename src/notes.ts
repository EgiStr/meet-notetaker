import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

/** The "transcript" kind of this event is handled by `useRecording`. */
type NotesEvent =
  | { kind: "mom"; data: string }
  | { kind: "pending"; data: string }
  | { kind: "questions"; data: string[] }
  | { kind: "brief"; data: string }
  | { kind: "title"; data: string }
  | { kind: "transcript" };

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

/** Calls `onTitle` when the app gave a meeting that had none a title from its minutes. */
export function useAutoTitle(onTitle: (title: string) => void) {
  useEffect(() => {
    const unlisten = listen<NotesEvent>("notes-event", ({ payload }) => {
      if (payload.kind === "title") onTitle(payload.data);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
}

/** The recap of what is being discussed, refreshed by the app while recording. */
export function useBrief(meetingId: number) {
  const [brief, setBrief] = useState("");

  useEffect(() => setBrief(""), [meetingId]);

  useEffect(() => {
    const unlisten = listen<NotesEvent>("notes-event", ({ payload }) => {
      if (payload.kind === "brief") setBrief(payload.data);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  return brief;
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

export interface Bookmark {
  at_secs: number;
  label: string;
}

export const TEMPLATES = [
  { id: "general", label: "Umum" },
  { id: "standup", label: "Standup" },
  { id: "one_on_one", label: "1:1" },
  { id: "sales", label: "Penjualan" },
  { id: "retro", label: "Retrospektif" },
  { id: "formal", label: "Formal (lengkap)" },
] as const;

export function setTemplate(template: string): Promise<void> {
  return invoke("set_template", { template });
}

export function addBookmark(label: string, atSecs: number | null): Promise<Bookmark[]> {
  return invoke("add_bookmark", { label, atSecs });
}

export function removeBookmark(index: number): Promise<Bookmark[]> {
  return invoke("remove_bookmark", { index });
}

/** The user's short notes written out with facts from the transcript; null while there are no notes. */
export function expandNotes(): Promise<string | null> {
  return invoke("expand_notes");
}

export function suggestQuestions(): Promise<string[]> {
  return invoke("suggest_questions");
}

/** A recap of the last `minutes` for someone who just joined; null before anyone spoke. */
export function catchUp(minutes: number): Promise<string | null> {
  return invoke("catch_up", { minutes });
}

/** A follow-up email draft for the participants; null before anyone spoke. */
export function followUpEmail(): Promise<string | null> {
  return invoke("follow_up_email");
}

/** What a transcript line means, from the conversation around it. */
export function explainLine(text: string, atSecs: number): Promise<string> {
  return invoke("explain_line", { text, atSecs });
}

export interface LibraryAnswer {
  answer: string;
  sources: { meeting_id: number; title: string; started_at: number }[];
}

export function askLibrary(question: string): Promise<LibraryAnswer> {
  return invoke("ask_library", { question, utcOffsetSecs: -new Date().getTimezoneOffset() * 60 });
}

/** Writes the current meeting to `path` as Markdown. */
export function exportMeeting(path: string): Promise<void> {
  return invoke("export_meeting", { path, utcOffsetSecs: -new Date().getTimezoneOffset() * 60 });
}

/** The meeting as a printable report document (HTML). */
export function reportHtml(): Promise<string> {
  return invoke("report_html", { utcOffsetSecs: -new Date().getTimezoneOffset() * 60 });
}

export interface TalkStat {
  name: string;
  seconds: number;
  share: number;
  lines: number;
  words: number;
  questions: number;
  longest_secs: number;
  words_per_minute: number;
}

/** Talk time and habits of each speaker of the open meeting. */
export function talkStats(): Promise<TalkStat[]> {
  return invoke("talk_stats");
}

/** Tells the voices of the open meeting apart again from its recording. */
export function relabelSpeakers(): Promise<void> {
  return invoke("relabel_speakers");
}

/** Returns whether the deleted meeting was the current one. */
export function deleteMeeting(id: number): Promise<boolean> {
  return invoke("delete_meeting", { id });
}

/** Questions worth asking about the meeting; the core sends new ones after every update of the minutes. */
export function useQuestions(meetingId: number) {
  const [questions, setQuestions] = useState<string[]>([]);

  useEffect(() => setQuestions([]), [meetingId]);

  useEffect(() => {
    const unlisten = listen<NotesEvent>("notes-event", ({ payload }) => {
      if (payload.kind === "questions") setQuestions(payload.data);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  return { questions, setQuestions };
}

/** The bookmarks of the current meeting. Changes go through the core, which keeps them in time order. */
export function useBookmarks(meetingId: number, saved: Bookmark[]) {
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);

  useEffect(() => {
    setBookmarks(saved);
    // Only a change of meeting resets the list; `saved` belongs to that meeting.
  }, [meetingId]);

  return {
    bookmarks,
    add: (label: string, atSecs: number | null) => addBookmark(label, atSecs).then(setBookmarks),
    remove: (index: number) => removeBookmark(index).then(setBookmarks),
  };
}
