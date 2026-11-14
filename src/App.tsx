import { useEffect, useRef, useState } from "react";
import { type Segment, type Source, type Status, useRecording } from "./recording";

const SPEAKER: Record<Source, { label: string; className: string }> = {
  microphone: { label: "Saya", className: "text-sky-700" },
  system: { label: "Lainnya", className: "text-amber-700" },
};

export function App() {
  const { status, segments, error, start, stop } = useRecording();
  const elapsed = useElapsedSeconds(status === "recording");

  return (
    <div className="flex h-full flex-col bg-zinc-50 text-zinc-900">
      <header className="flex items-center gap-4 border-b border-zinc-200 bg-white px-5 py-3">
        <RecordButton status={status} onStart={start} onStop={stop} />
        <span className="font-mono text-sm tabular-nums text-zinc-600">{formatClock(elapsed)}</span>
        {status === "recording" && (
          <span className="text-sm text-zinc-600">Merekam mikrofon dan audio sistem. Beri tahu peserta lain.</span>
        )}
        {status === "stopping" && <span className="text-sm text-zinc-600">Menyelesaikan transkripsi…</span>}
      </header>

      {error && (
        <p role="alert" className="border-b border-red-200 bg-red-50 px-5 py-2 text-sm text-red-800">
          {error}
        </p>
      )}

      <Transcript segments={segments} status={status} />
    </div>
  );
}

function RecordButton(props: { status: Status; onStart: () => void; onStop: () => void }) {
  if (props.status === "idle") {
    return (
      <button
        onClick={props.onStart}
        className="rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700"
      >
        Rekam
      </button>
    );
  }
  return (
    <button
      onClick={props.onStop}
      disabled={props.status === "stopping"}
      className="rounded-md bg-zinc-800 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-900 disabled:opacity-50"
    >
      Berhenti
    </button>
  );
}

function Transcript({ segments, status }: { segments: Segment[]; status: Status }) {
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [segments.length]);

  if (segments.length === 0) {
    return (
      <main className="flex flex-1 items-center justify-center px-5 text-sm text-zinc-500">
        {status === "idle"
          ? "Tekan Rekam untuk mulai. Transkrip muncul beberapa detik setelah diucapkan."
          : "Menunggu ucapan…"}
      </main>
    );
  }

  return (
    <main className="flex-1 overflow-y-auto px-5 py-4">
      <ol className="mx-auto flex max-w-3xl flex-col gap-3">
        {segments.map((segment) => (
          <li key={`${segment.source}-${segment.start_secs}`} className="grid grid-cols-[3.5rem_5rem_1fr] gap-3 text-sm">
            <time className="font-mono tabular-nums text-zinc-500">{formatClock(segment.start_secs)}</time>
            <span className={`font-medium ${SPEAKER[segment.source].className}`}>{SPEAKER[segment.source].label}</span>
            <p className="leading-relaxed">{segment.text}</p>
          </li>
        ))}
      </ol>
      <div ref={end} />
    </main>
  );
}

function useElapsedSeconds(running: boolean) {
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (!running) return;
    const startedAt = Date.now();
    setElapsed(0);
    const timer = setInterval(() => setElapsed((Date.now() - startedAt) / 1000), 500);
    return () => clearInterval(timer);
  }, [running]);
  return elapsed;
}

function formatClock(totalSeconds: number) {
  const whole = Math.floor(totalSeconds);
  const minutes = String(Math.floor(whole / 60)).padStart(2, "0");
  const seconds = String(whole % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
}
