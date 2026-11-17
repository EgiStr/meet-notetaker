import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { AskPanel } from "./AskPanel";
import { History } from "./History";
import { MomPanel } from "./MomPanel";
import { ModelPicker, Models } from "./Models";
import { mentions, parseNames } from "./mentions";
import { useModels } from "./providers";
import {
  TEMPLATES,
  catchUp,
  exportMeeting,
  explainLine,
  followUpEmail,
  rebuildMom,
  setTemplate as applyTemplate,
  useAutoTitle,
  useBookmarks,
  useBrief,
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

/** A short soft tone, so a mention is noticed without looking at the screen. */
function beep() {
  try {
    const audio = new AudioContext();
    const tone = audio.createOscillator();
    const volume = audio.createGain();
    volume.gain.value = 0.08;
    tone.frequency.value = 880;
    tone.connect(volume).connect(audio.destination);
    tone.start();
    tone.stop(audio.currentTime + 0.25);
    tone.onended = () => void audio.close();
  } catch {
    // No sound available; the banner still shows.
  }
}

const SPEAKER_COLOR:Record<Source, string> = {
  microphone: "text-sky-700",
  system: "text-amber-700",
};

export function App() {
  const { status, segments, error, meetingId, names, opened, start, stop, open, rename, importFile, clear } =
    useRecording();
  const { mom, pending } = useMom(meetingId, opened?.mom ?? "");
  const { questions, setQuestions } = useQuestions(meetingId);
  const brief = useBrief(meetingId);
  const { bookmarks, add: addBookmark, remove: removeBookmark } = useBookmarks(meetingId, opened?.bookmarks ?? []);
  const [title, setTitle] = useState("");
  useAutoTitle(setTitle);
  const [template, setTemplate] = useState("general");
  const [showHistory, setShowHistory] = useState(false);
  const [showModels, setShowModels] = useState(false);
  const modelSettings = useModels();
  /** The outcome of the last action that is not part of recording, shown like `error`. */
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (opened) {
      setTitle(opened.title);
      setTemplate(opened.template);
    }
  }, [opened]);

  const [myName, setMyName] = useState(() => {
    try {
      return localStorage.getItem("myName") ?? "";
    } catch {
      return "";
    }
  });
  const myNames = parseNames(myName);
  /** The latest line from someone else that said one of the user's names. */
  const [mention, setMention] = useState<Segment | null>(null);
  const checked = useRef(new Set<string>());

  useEffect(() => {
    try {
      localStorage.setItem("myName", myName);
    } catch {
      // The name is only a convenience; it is just not remembered.
    }
  }, [myName]);

  useEffect(() => {
    // Lines are inserted by start time, so what is new is found by key, not by position.
    const fresh = segments.filter((segment) => !checked.current.has(`${segment.source}-${segment.start_secs}`));
    checked.current = new Set(segments.map((segment) => `${segment.source}-${segment.start_secs}`));
    if (status !== "recording") return;
    const said = fresh.filter((segment) => segment.source === "system" && mentions(segment.text, myNames)).at(-1);
    if (said) {
      setMention(said);
      beep();
    }
  }, [segments]);

  useEffect(() => setMention(null), [meetingId]);

  const [recapMinutes, setRecapMinutes] = useState(5);
  const [recap, setRecap] = useState<"loading" | { text: string; ok: boolean } | null>(null);

  async function runCatchUp() {
    setRecap("loading");
    try {
      setRecap({ text: (await catchUp(recapMinutes)) ?? "Belum ada ucapan untuk diringkas.", ok: true });
    } catch (reason) {
      setRecap({ text: String(reason), ok: false });
    }
  }

  const [email, setEmail] = useState<"loading" | { text: string; ok: boolean } | null>(null);

  async function draftEmail() {
    setEmail("loading");
    try {
      setEmail({ text: (await followUpEmail()) ?? "Belum ada ucapan untuk dijadikan email.", ok: true });
    } catch (reason) {
      setEmail({ text: String(reason), ok: false });
    }
  }

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
        <input
          value={myName}
          onChange={(event) => setMyName(event.target.value)}
          placeholder="Nama saya"
          title="Nama yang Anda pakai di rapat; pisahkan dengan koma. Anda diberi tahu saat ada yang menyebutnya."
          aria-label="Nama saya"
          className="w-28 rounded-md border border-zinc-300 px-2 py-1.5 text-sm outline-none focus:border-zinc-500"
        />
        <select
          value={recapMinutes}
          onChange={(event) => setRecapMinutes(Number(event.target.value))}
          aria-label="Rentang untuk Susul saya"
          className="rounded-md border border-zinc-300 px-2 py-1.5 text-sm"
        >
          {[3, 5, 10, 30].map((minutes) => (
            <option key={minutes} value={minutes}>
              {minutes} menit
            </option>
          ))}
        </select>
        <button
          onClick={() => void runCatchUp()}
          disabled={meetingId === 0 || segments.length === 0 || recap === "loading"}
          title="Ringkas apa yang baru saja dibahas, supaya langsung paham"
          className="rounded-md bg-indigo-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-800 disabled:opacity-50"
        >
          Susul saya
        </button>
        {status === "recording" && (
          <button onClick={markNow} className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50">
            Tandai
          </button>
        )}
        <button
          onClick={() => void draftEmail()}
          disabled={status !== "idle" || meetingId === 0 || segments.length === 0 || email === "loading"}
          title="Tulis draf email tindak lanjut untuk peserta"
          className="ml-auto rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
        >
          Draf email
        </button>
        <button
          onClick={() => void exportToFile()}
          disabled={status !== "idle" || meetingId === 0}
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
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
        <ModelPicker models={modelSettings.models} apply={modelSettings.apply} />
        <button
          onClick={() => {
            setShowModels((shown) => !shown);
            setShowHistory(false);
          }}
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50"
        >
          {showModels ? "Kembali" : "Model"}
        </button>
        <button
          onClick={() => {
            setShowHistory((shown) => !shown);
            setShowModels(false);
          }}
          disabled={status !== "idle"}
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
        >
          {showHistory ? "Kembali" : "Riwayat"}
        </button>
      </header>

      {mention && (
        <section role="alert" className="flex items-start gap-3 border-b border-amber-300 bg-amber-50 px-5 py-3 text-sm text-amber-950">
          <p className="min-w-0 flex-1 leading-relaxed">
            <strong>Nama Anda disebut</strong> pukul {formatClock(mention.start_secs)}: “{mention.text}”
          </p>
          <button
            onClick={() => {
              setMention(null);
              void runCatchUp();
            }}
            className="rounded-md border border-amber-400 px-2 py-0.5 hover:bg-amber-100"
          >
            Susul saya
          </button>
          <button onClick={() => setMention(null)} className="text-amber-800 hover:text-amber-950">
            Tutup
          </button>
        </section>
      )}

      {status === "recording" && brief && (
        <details open className="border-b border-indigo-200 bg-indigo-50 px-5 py-2 text-sm text-indigo-950">
          <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-indigo-700">
            Sekarang · diperbarui otomatis tiap menit
          </summary>
          <p className="mt-1 whitespace-pre-wrap leading-relaxed">{brief}</p>
        </details>
      )}

      {recap && (
        <section className="flex items-start gap-3 border-b border-indigo-200 bg-indigo-50 px-5 py-3 text-sm text-indigo-950">
          <div className="min-w-0 flex-1">
            <p className="mb-1 text-xs font-semibold uppercase tracking-wide text-indigo-700">
              Susul saya · {recapMinutes} menit terakhir
            </p>
            {recap === "loading" ? (
              <p>Membaca percakapan…</p>
            ) : (
              <p className={`whitespace-pre-wrap leading-relaxed ${recap.ok ? "" : "text-red-700"}`}>{recap.text}</p>
            )}
          </div>
          <button onClick={() => setRecap(null)} aria-label="Tutup ringkasan" className="text-indigo-700 hover:text-indigo-950">
            Tutup
          </button>
        </section>
      )}

      {email && (
        <section className="flex items-start gap-3 border-b border-zinc-200 bg-zinc-50 px-5 py-3 text-sm">
          <div className="min-w-0 flex-1">
            <p className="mb-1 text-xs font-semibold uppercase tracking-wide text-zinc-500">Draf email tindak lanjut</p>
            {email === "loading" ? (
              <p>Menulis draf…</p>
            ) : (
              <p className={`max-h-64 overflow-y-auto whitespace-pre-wrap leading-relaxed ${email.ok ? "" : "text-red-700"}`}>
                {email.text}
              </p>
            )}
          </div>
          {email !== "loading" && email.ok && (
            <button
              onClick={() => void navigator.clipboard.writeText(email.text).catch((reason) => setNotice(String(reason)))}
              className="rounded-md border border-zinc-300 px-2 py-0.5 hover:bg-zinc-100"
            >
              Salin
            </button>
          )}
          <button onClick={() => setEmail(null)} aria-label="Tutup draf email" className="text-zinc-500 hover:text-zinc-900">
            Tutup
          </button>
        </section>
      )}

      {(error ?? notice) && (
        <p role="alert" className="border-b border-red-200 bg-red-50 px-5 py-2 text-sm text-red-800">
          {error ?? notice}
        </p>
      )}

      {showModels ? (
        <Models models={modelSettings.models} error={modelSettings.error} apply={modelSettings.apply} />
      ) : showHistory ? (
        <History onOpen={(id) => void openFromHistory(id)} onDeletedCurrent={clear} />
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[1fr_1fr_18rem]">
          <Transcript
            segments={segments}
            status={status}
            names={names}
            highlight={myNames}
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
  /** Lines from others that say one of these names are marked. */
  highlight: string[];
  onRename: (speaker: number, name: string) => void;
  onBookmark: (segment: Segment) => void;
}

function Transcript({ segments, status, names, highlight, onRename, onBookmark }: TranscriptProps) {
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [segments.length]);

  /** What the assistant said about a line, by the line's start time. */
  const [explanations, setExplanations] = useState<Record<number, { text: string; ok: boolean }>>({});

  async function explain(segment: Segment) {
    const at = segment.start_secs;
    setExplanations((current) => ({ ...current, [at]: { text: "Menjelaskan…", ok: true } }));
    try {
      const text = await explainLine(segment.text, at);
      setExplanations((current) => ({ ...current, [at]: { text, ok: true } }));
    } catch (reason) {
      setExplanations((current) => ({ ...current, [at]: { text: String(reason), ok: false } }));
    }
  }

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
            className="group grid grid-cols-[3rem_5.5rem_1fr_3.5rem] gap-3 text-sm"
          >
            <time className="font-mono tabular-nums text-zinc-500">{formatClock(segment.start_secs)}</time>
            <span className={`break-words font-medium ${SPEAKER_COLOR[segment.source]}`}>
              {speakerLabel(segment, names)}
            </span>
            <div>
              <p
                className={`leading-relaxed ${
                  segment.source === "system" && mentions(segment.text, highlight) ? "rounded bg-amber-100 px-1" : ""
                }`}
              >
                {segment.text}
              </p>
              {explanations[segment.start_secs] && (
                <p
                  className={`mt-1 whitespace-pre-wrap rounded-md border px-3 py-2 leading-relaxed ${
                    explanations[segment.start_secs].ok
                      ? "border-indigo-200 bg-indigo-50 text-indigo-950"
                      : "border-red-200 bg-red-50 text-red-800"
                  }`}
                >
                  {explanations[segment.start_secs].text}
                </p>
              )}
            </div>
            <span className="flex items-start gap-2 opacity-0 focus-within:opacity-100 group-hover:opacity-100">
              <button
                onClick={() => void explain(segment)}
                aria-label={`Jelaskan ${formatClock(segment.start_secs)}`}
                title="Jelaskan ucapan ini"
                className="font-medium text-indigo-600 hover:text-indigo-900"
              >
                ?
              </button>
              <button
                onClick={() => onBookmark(segment)}
                aria-label={`Tandai ${formatClock(segment.start_secs)}`}
                title="Tandai bagian ini"
                className="text-zinc-400 hover:text-amber-600"
              >
                ☆
              </button>
            </span>
          </li>
        ))}
      </ol>
      <div ref={end} />
    </main>
  );
}

/** Shown once the final diarization has numbered the speakers. */
function SpeakerNamesForm({ segments, names, onRename }: Omit<TranscriptProps, "status" | "highlight" | "onBookmark">) {
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
