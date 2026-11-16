import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { AskPanel } from "./AskPanel";
import { History } from "./History";
import { MomPanel } from "./MomPanel";
import {
  TEMPLATES,
  exportMeeting,
  rebuildMom,
  setTemplate as applyTemplate,
  useBookmarks,
  useMom,
  useQuestions,
} from "./notes";
import {
  type Segment,
  type Source,
  type SpeakerNames,
  type Status,
  formatClock,
  speakerLabel,
  useRecording,
} from "./recording";

const SPEAKER_COLOR: Record<Source, string> = {
  microphone: "text-sky-700",
  system: "text-amber-700",
};

export function App() {
  const { status, segments, error, meetingId, names, opened, start, stop, open, rename, importFile, clear } =
    useRecording();
  const { mom, pending } = useMom(meetingId, opened?.mom ?? "");
  const { questions, setQuestions } = useQuestions(meetingId);
  const { bookmarks, add: addBookmark, remove: removeBookmark } = useBookmarks(meetingId, opened?.bookmarks ?? []);
  const [title, setTitle] = useState("");
  const [template, setTemplate] = useState("general");
  const [showHistory, setShowHistory] = useState(false);
  /** The outcome of the last action that is not part of recording, shown like `error`. */
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (opened) {
      setTitle(opened.title);
      setTemplate(opened.template);
    }
  }, [opened]);

  function report(action: Promise<unknown>) {
    setNotice(null);
    action.catch((reason) => setNotice(String(reason)));
  }

  /** Applies to the minutes from now on; a finished meeting gets its minutes rewritten right away. */
  function changeTemplate(next: string) {
    setTemplate(next);
    if (meetingId === 0) return;
    report(applyTemplate(next).then(() => (status === "idle" ? rebuildMom() : undefined)));
  }

  /** Marks the moment the recording has reached, titled with what was said last. */
  function markNow() {
    report(addBookmark(segments.at(-1)?.text.slice(0, 80) ?? "", null));
  }

  async function exportToFile() {
    const path = await save({
      defaultPath: `${title.trim() || "rapat"}.md`,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (path) report(exportMeeting(path));
  }

  async function importRecording() {
    setShowHistory(false);
    const name = await importFile();
    if (name) setTitle(name);
  }

  async function openFromHistory(id: number) {
    await open(id);
    setShowHistory(false);
  }
  const elapsed = useElapsedSeconds(status === "recording");

  return (
    <div className="flex h-full flex-col bg-zinc-50 text-zinc-900">
      <header className="flex items-center gap-4 border-b border-zinc-200 bg-white px-5 py-3">
        <RecordButton status={status} onStart={() => void start(title, template)} onStop={stop} />
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
        {status === "importing" && (
          <span className="text-sm text-zinc-600">Mengimpor: transkripsi, label pembicara, lalu MoM…</span>
        )}
        <select
          value={template}
          onChange={(event) => changeTemplate(event.target.value)}
          disabled={status === "stopping" || status === "importing"}
          aria-label="Template MoM"
          className="rounded-md border border-zinc-300 px-2 py-1.5 text-sm"
        >
          {TEMPLATES.map((option) => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </select>
        {status === "recording" && (
          <button onClick={markNow} className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50">
            Tandai
          </button>
        )}
        <button
          onClick={() => void exportToFile()}
          disabled={status !== "idle" || meetingId === 0}
          className="ml-auto rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
        >
          Ekspor
        </button>
        <button
          onClick={() => void importRecording()}
          disabled={status !== "idle"}
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
        >
          Impor
        </button>
        <button
          onClick={() => setShowHistory((shown) => !shown)}
          disabled={status !== "idle"}
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
        >
          {showHistory ? "Kembali" : "Riwayat"}
        </button>
      </header>

      {(error ?? notice) && (
        <p role="alert" className="border-b border-red-200 bg-red-50 px-5 py-2 text-sm text-red-800">
          {error ?? notice}
        </p>
      )}

      {showHistory ? (
        <History onOpen={(id) => void openFromHistory(id)} onDeletedCurrent={clear} />
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[1fr_1fr_18rem]">
          <Transcript
            segments={segments}
            status={status}
            names={names}
            onRename={rename}
            onBookmark={(segment) => report(addBookmark(segment.text.slice(0, 80), segment.start_secs))}
          />
          <MomPanel mom={mom} pending={pending} canRetry={status === "idle"} />
          <AskPanel
            key={meetingId}
            enabled={meetingId > 0}
            savedNotes={opened?.userNotes ?? ""}
            savedChat={opened?.chat ?? []}
            bookmarks={bookmarks}
            onRemoveBookmark={(index) => report(removeBookmark(index))}
            suggestions={questions}
            onSuggestions={setQuestions}
          />
        </div>
      )}
    </div>
  );
}

function RecordButton(props: { status: Status; onStart: () => void; onStop: () => void }) {
  if (props.status === "idle" || props.status === "importing") {
    return (
      <button
        onClick={props.onStart}
        disabled={props.status === "importing"}
        className="rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50"
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
  onBookmark: (segment: Segment) => void;
}

function Transcript({ segments, status, names, onRename, onBookmark }: TranscriptProps) {
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [segments.length]);

  if (segments.length === 0) {
    return (
      <main className="flex items-center justify-center border-r border-zinc-200 px-5 text-center text-sm text-zinc-500">
        {status === "idle"
          ? "Tekan Rekam untuk mulai, atau Impor untuk memproses file rekaman."
          : "Menunggu ucapan…"}
      </main>
    );
  }

  return (
    <main className="min-h-0 overflow-y-auto border-r border-zinc-200 px-4 py-4">
      {status === "idle" && <SpeakerNamesForm segments={segments} names={names} onRename={onRename} />}
      <ol className="mx-auto flex max-w-3xl flex-col gap-3">
        {segments.map((segment) => (
          <li
            key={`${segment.source}-${segment.start_secs}`}
            className="group grid grid-cols-[3rem_5.5rem_1fr_1.5rem] gap-3 text-sm"
          >
            <time className="font-mono tabular-nums text-zinc-500">{formatClock(segment.start_secs)}</time>
            <span className={`break-words font-medium ${SPEAKER_COLOR[segment.source]}`}>
              {speakerLabel(segment, names)}
            </span>
            <p className="leading-relaxed">{segment.text}</p>
            <button
              onClick={() => onBookmark(segment)}
              aria-label={`Tandai ${formatClock(segment.start_secs)}`}
              title="Tandai bagian ini"
              className="text-zinc-300 opacity-0 hover:text-amber-600 focus:opacity-100 group-hover:opacity-100"
            >
              ☆
            </button>
          </li>
        ))}
      </ol>
      <div ref={end} />
    </main>
  );
}

/** Shown once the final diarization has numbered the speakers. */
function SpeakerNamesForm({ segments, names, onRename }: Omit<TranscriptProps, "status" | "onBookmark">) {
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
