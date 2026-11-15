import { useEffect, useRef, useState } from "react";
import { AskPanel } from "./AskPanel";
import { History } from "./History";
import { MomPanel } from "./MomPanel";
import { useMom } from "./notes";
import { type Segment, type Source, type SpeakerNames, type Status, speakerLabel, useRecording } from "./recording";

const SPEAKER_COLOR: Record<Source, string> = {
  microphone: "text-sky-700",
  system: "text-amber-700",
};

export function App() {
  const { status, segments, error, meetingId, names, opened, start, stop, open, rename } = useRecording();
  const { mom, pending } = useMom(meetingId, opened?.mom ?? "");
  const [title, setTitle] = useState("");
  const [showHistory, setShowHistory] = useState(false);

  useEffect(() => {
    if (opened) setTitle(opened.title);
  }, [opened]);

  async function openFromHistory(id: number) {
    await open(id);
    setShowHistory(false);
  }
  const elapsed = useElapsedSeconds(status === "recording");

  return (
    <div className="flex h-full flex-col bg-zinc-50 text-zinc-900">
      <header className="flex items-center gap-4 border-b border-zinc-200 bg-white px-5 py-3">
        <RecordButton status={status} onStart={() => void start(title)} onStop={stop} />
        <input
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          disabled={status !== "idle"}
          placeholder="Judul rapat"
          aria-label="Judul rapat"
          className="w-56 rounded-md border border-zinc-300 px-3 py-1.5 text-sm outline-none focus:border-zinc-500 disabled:bg-zinc-50"
        />
        <span className="font-mono text-sm tabular-nums text-zinc-600">{formatClock(elapsed)}</span>
        {status === "recording" && (
          <span className="text-sm text-zinc-600">Merekam mikrofon dan audio sistem. Beri tahu peserta lain.</span>
        )}
        {status === "stopping" && <span className="text-sm text-zinc-600">Menyelesaikan transkripsi, label pembicara, dan MoM akhir…</span>}
        <button
          onClick={() => setShowHistory((shown) => !shown)}
          disabled={status !== "idle"}
          className="ml-auto rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
        >
          {showHistory ? "Kembali" : "Riwayat"}
        </button>
      </header>

      {error && (
        <p role="alert" className="border-b border-red-200 bg-red-50 px-5 py-2 text-sm text-red-800">
          {error}
        </p>
      )}

      {showHistory ? (
        <History onOpen={(id) => void openFromHistory(id)} />
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[1fr_1fr_18rem]">
          <Transcript segments={segments} status={status} names={names} onRename={rename} />
          <MomPanel mom={mom} pending={pending} canRetry={status === "idle"} />
          <AskPanel
            key={meetingId}
            enabled={meetingId > 0}
            savedNotes={opened?.userNotes ?? ""}
            savedChat={opened?.chat ?? []}
          />
        </div>
      )}
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

interface TranscriptProps {
  segments: Segment[];
  status: Status;
  names: SpeakerNames;
  onRename: (speaker: number, name: string) => void;
}

function Transcript({ segments, status, names, onRename }: TranscriptProps) {
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [segments.length]);

  if (segments.length === 0) {
    return (
      <main className="flex items-center justify-center border-r border-zinc-200 px-5 text-center text-sm text-zinc-500">
        {status === "idle"
          ? "Tekan Rekam untuk mulai. Transkrip muncul beberapa detik setelah diucapkan."
          : "Menunggu ucapan…"}
      </main>
    );
  }

  return (
    <main className="min-h-0 overflow-y-auto border-r border-zinc-200 px-4 py-4">
      {status === "idle" && <SpeakerNamesForm segments={segments} names={names} onRename={onRename} />}
      <ol className="mx-auto flex max-w-3xl flex-col gap-3">
        {segments.map((segment) => (
          <li key={`${segment.source}-${segment.start_secs}`} className="grid grid-cols-[3rem_5.5rem_1fr] gap-3 text-sm">
            <time className="font-mono tabular-nums text-zinc-500">{formatClock(segment.start_secs)}</time>
            <span className={`break-words font-medium ${SPEAKER_COLOR[segment.source]}`}>
              {speakerLabel(segment, names)}
            </span>
            <p className="leading-relaxed">{segment.text}</p>
          </li>
        ))}
      </ol>
      <div ref={end} />
    </main>
  );
}

/** Shown once the final diarization has numbered the speakers. */
function SpeakerNamesForm({ segments, names, onRename }: Omit<TranscriptProps, "status">) {
  const speakers = [...new Set(segments.flatMap((segment) => segment.speaker ?? []))].sort((a, b) => a - b);
  if (speakers.length === 0) return null;

  return (
    <fieldset className="mx-auto mb-4 flex max-w-3xl flex-wrap gap-2 border-b border-zinc-200 pb-4 text-sm">
      <legend className="mb-2 text-xs font-semibold uppercase tracking-wide text-zinc-500">Nama pembicara</legend>
      {speakers.map((speaker) => (
        <input
          key={speaker}
          aria-label={`Nama untuk Pembicara ${speaker}`}
          defaultValue={names[speaker] ?? ""}
          placeholder={`Pembicara ${speaker}`}
          onBlur={(event) => {
            if (event.target.value.trim() !== (names[speaker] ?? "")) onRename(speaker, event.target.value);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
          className="w-36 rounded-md border border-zinc-300 px-2 py-1 outline-none focus:border-zinc-500"
        />
      ))}
    </fieldset>
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
