import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export type Source = "microphone" | "system";

export interface Segment {
  source: Source;
  start_secs: number;
  end_secs: number;
  text: string;
}

type SessionEvent = { kind: "segment"; data: Segment } | { kind: "error"; data: string };

export type Status = "idle" | "recording" | "stopping";

/** Owns the recording lifecycle and the transcript received from the Rust core. */
export function useRecording() {
  const [status, setStatus] = useState<Status>("idle");
  const [segments, setSegments] = useState<Segment[]>([]);
  const [error, setError] = useState<string | null>(null);
  /** Increases with every recording that starts; 0 means there is no meeting yet. */
  const [meetingId, setMeetingId] = useState(0);

  useEffect(() => {
    const unlisten = listen<SessionEvent>("session-event", ({ payload }) => {
      if (payload.kind === "segment") {
        setSegments((current) => insertByStart(current, payload.data));
      } else {
        setError(payload.data);
      }
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  async function start() {
    setError(null);
    setSegments([]);
    try {
      await invoke("start_recording");
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

  return { status, segments, error, meetingId, start, stop };
}

/** The two streams are transcribed independently, so segments can arrive out of order. */
export function insertByStart(segments: Segment[], incoming: Segment): Segment[] {
  const index = segments.findIndex((segment) => segment.start_secs > incoming.start_secs);
  if (index === -1) return [...segments, incoming];
  return [...segments.slice(0, index), incoming, ...segments.slice(index)];
}
