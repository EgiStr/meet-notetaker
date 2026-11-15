import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as pickFile } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

export type Source = "microphone" | "system";

export interface Segment {
  source: Source;
  start_secs: number;
  end_secs: number;
  text: string;
  /** Set by the final diarization: 1 for the first voice heard in the system audio, and so on. */
  speaker: number | null;
}

/** Names the user gave to speakers, by speaker number. */
export type SpeakerNames = Record<number, string>;

export function speakerLabel(segment: Segment, names: SpeakerNames): string {
  if (segment.source === "microphone") return "Saya";
  if (segment.speaker === null) return "Lainnya";
  return names[segment.speaker] || `Pembicara ${segment.speaker}`;
}

/** What a saved meeting brings back besides its transcript. */
export interface OpenedMeeting {
  title: string;
  mom: string;
  userNotes: string;
  chat: { question: string; answer: string }[];
}

interface MeetingView {
  summary: { title: string };
  notes: { segments: Segment[]; names: SpeakerNames; mom: string; user_notes: string; chat: OpenedMeeting["chat"] };
}

type SessionEvent = { kind: "segment"; data: Segment } | { kind: "error"; data: string };

export type Status = "idle" | "recording" | "stopping" | "importing";

const AUDIO_EXTENSIONS = ["wav", "mp3", "m4a", "aac", "flac", "ogg"];

/** Owns the recording lifecycle and the transcript received from the Rust core. */
export function useRecording() {
  const [status, setStatus] = useState<Status>("idle");
  const [segments, setSegments] = useState<Segment[]>([]);
  const [error, setError] = useState<string | null>(null);
  /** Increases with every recording that starts; 0 means there is no meeting yet. */
  const [meetingId, setMeetingId] = useState(0);
  const [names, setNames] = useState<SpeakerNames>({});
  /** Set while the current meeting was loaded from the history, not recorded just now. */
  const [opened, setOpened] = useState<OpenedMeeting | null>(null);

  useEffect(() => {
    const unlisten = listen<SessionEvent>("session-event", ({ payload }) => {
      if (payload.kind === "segment") {
        setSegments((current) => insertByStart(current, payload.data));
      } else {
        setError(payload.data);
      }
    });
    // After the meeting the core sends the transcript again with final speaker labels.
    const unlistenLabels = listen<{ kind: string; data: unknown }>("notes-event", ({ payload }) => {
      if (payload.kind === "transcript") setSegments(payload.data as Segment[]);
    });
    return () => {
      void unlisten.then((stop) => stop());
      void unlistenLabels.then((stop) => stop());
    };
  }, []);

  async function start(title: string) {
    setError(null);
    try {
      await invoke("start_recording", { title });
      setSegments([]);
      setNames({});
      setOpened(null);
      setStatus("recording");
      setMeetingId((current) => current + 1);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function stop() {
    setStatus("stopping");
    try {
      await invoke("stop_recording");
    } catch (reason) {
      setError(String(reason));
    }
    setStatus("idle");
  }

  /** Lets the user pick a recording and processes it. Returns its file name, or null if none was picked. */
  async function importFile(): Promise<string | null> {
    const path = await pickFile({ multiple: false, filters: [{ name: "Rekaman audio", extensions: AUDIO_EXTENSIONS }] });
    if (typeof path !== "string") return null;
    setError(null);
    setSegments([]);
    setNames({});
    setOpened(null);
    setMeetingId((current) => current + 1);
    setStatus("importing");
    try {
      await invoke("import_recording", { path });
    } catch (reason) {
      setError(String(reason));
    }
    setStatus("idle");
    return path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? null;
  }

  async function open(id: number) {
    setError(null);
    try {
      const { summary, notes } = await invoke<MeetingView>("open_meeting", { id });
      setSegments(notes.segments);
      setNames(notes.names);
      setOpened({ title: summary.title, mom: notes.mom, userNotes: notes.user_notes, chat: notes.chat });
      setMeetingId((current) => current + 1);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function rename(speaker: number, name: string) {
    try {
      await invoke("rename_speaker", { speaker, name });
      setNames((current) => ({ ...current, [speaker]: name.trim() }));
    } catch (reason) {
      setError(String(reason));
    }
  }

  return { status, segments, error, meetingId, names, opened, start, stop, open, rename, importFile };
}

/** The two streams are transcribed independently, so segments can arrive out of order. */
export function insertByStart(segments: Segment[], incoming: Segment): Segment[] {
  const index = segments.findIndex((segment) => segment.start_secs > incoming.start_secs);
  if (index === -1) return [...segments, incoming];
  return [...segments.slice(0, index), incoming, ...segments.slice(index)];
}
