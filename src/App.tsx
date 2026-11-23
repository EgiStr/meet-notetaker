import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { confirm, save } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { AskPanel, NotesPanel } from "./AskPanel";
import { History } from "./History";
import { MomPanel } from "./MomPanel";
import { ModelPicker, Models } from "./Models";
import { mentions, parseNames } from "./mentions";
import { useModels } from "./providers";
import { notifyAway, quitApp, useAutostart, useMeetingDetected, useShowSummary, useTray } from "./tray";
import {
  TEMPLATES,
  catchUp,
  exportMeeting,
  reportHtml,
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

const SPEAKER_COLOR: Record<Source, string> = {
  microphone: "text-indigo-700",
  system: "text-zinc-700",
};

type View = "meeting" | "history" | "models";
type Panel = "summary" | "ask" | "notes";

const PANELS: { id: Panel; label: string }[] = [
  { id: "summary", label: "Ringkasan" },
  { id: "ask", label: "Tanya" },
  { id: "notes", label: "Catatan" },
];

const LANGUAGES: [string, string][] = [
  ["id", "Bahasa Indonesia"],
  ["en", "English"],
  ["ms", "Bahasa Melayu"],
  ["jw", "Basa Jawa"],
  ["su", "Basa Sunda"],
  ["zh", "中文"],
  ["ja", "日本語"],
  ["ar", "العربية"],
];

const BUTTON = "rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50";

export function App() {
  const { status, segments, error, meetingId, names, opened, start, stop, open, rename, edit, importFile, clear } =
    useRecording();
  const { mom, pending } = useMom(meetingId, opened?.mom ?? "");
  const { questions, setQuestions } = useQuestions(meetingId);
  const brief = useBrief(meetingId);
  const { bookmarks, add: addBookmark, remove: removeBookmark } = useBookmarks(meetingId, opened?.bookmarks ?? []);
  const [title, setTitle] = useState("");
  useAutoTitle(setTitle);
  const [template, setTemplate] = useState("general");
  const [view, setView] = useState<View>("meeting");
  const [panel, setPanel] = useState<Panel>("summary");
  const modelSettings = useModels();
  const autostart = useAutostart();
  const [language, setLanguage] = useState("id");
  const [glossary, setGlossary] = useState("");
  /** The saved audio of this meeting, by stream, so a transcript line can be played. */
  const [audioFiles, setAudioFiles] = useState<Record<string, string>>({});
  useEffect(() => {
    if (status === "recording" || status === "stopping" || status === "importing") return;
    invoke<Record<string, string>>("recording_files").then(setAudioFiles, () => setAudioFiles({}));
  }, [meetingId, status]);
  useEffect(() => {
    invoke<string>("get_language").then(setLanguage, () => {});
    invoke<string>("get_glossary").then(setGlossary, () => {});
  }, []);
  /** The outcome of the last action that is not part of recording, shown like `error`. */
  const [notice, setNotice] = useState<string | null>(null);

  useTray({
    recording: status === "recording",
    onToggleRecording: () => {
      if (status === "idle") void start(title, template);
      else if (status === "recording") void stop();
    },
    onQuit: async () => {
      if (status === "stopping" || status === "importing") {
        setNotice("Tunggu sampai prosesnya selesai sebelum keluar.");
        return;
      }
      if (status === "recording") {
        const sure = await confirm("Rekaman masih berjalan. Hentikan dan keluar?", { title: "Keluar", kind: "warning" });
        if (!sure) return;
        await stop();
      }
      await quitApp();
    },
  });

  /** An online meeting that opened while nothing is being recorded. */
  const [detected, setDetected] = useState<string | null>(null);
  const [autoRecord, setAutoRecord] = useState(() => {
    try {
      return localStorage.getItem("autoRecord") === "1";
    } catch {
      return false;
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem("autoRecord", autoRecord ? "1" : "0");
    } catch {
      // Only the setting is not remembered.
    }
  }, [autoRecord]);
  useMeetingDetected((name) => {
    if (status !== "idle") return;
    if (autoRecord) {
      void start(title, template);
      notifyAway(`Merekam rapat ${name}`, "Tekan Ctrl+Alt+R untuk berhenti.");
    } else {
      setDetected(name);
      notifyAway(`${name} terdeteksi`, "Tekan Ctrl+Alt+R atau pilih Mulai merekam di ikon tray.");
    }
  });
  useShowSummary(() => {
    setView("meeting");
    setPanel("summary");
  });
  useEffect(() => {
    if (status !== "idle") setDetected(null);
  }, [status]);

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
      notifyAway("Nama Anda disebut", `${formatClock(said.start_secs)}: ${said.text}`);
    }
  }, [segments]);

  useEffect(() => setMention(null), [meetingId]);

  const [recapMinutes, setRecapMinutes] = useState(5);
  const [recap, setRecap] = useState<"loading" | { text: string; ok: boolean } | null>(null);

  async function runCatchUp(minutes = recapMinutes) {
    setRecapMinutes(minutes);
    setRecap("loading");
    try {
      setRecap({ text: (await catchUp(minutes)) ?? "Belum ada ucapan untuk diringkas.", ok: true });
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

  /** The file type picked in the dialog decides what is written: md, html (report), txt or json (raw). */
  async function exportToFile(kind: "md" | "html" | "raw") {
    const filters = {
      md: [{ name: "Markdown", extensions: ["md"] }],
      html: [{ name: "Laporan MoM (HTML)", extensions: ["html"] }],
      raw: [
        { name: "Transkrip mentah (teks)", extensions: ["txt"] },
        { name: "Data mentah (JSON)", extensions: ["json"] },
      ],
    }[kind];
    const path = await save({ defaultPath: `${title.trim() || "rapat"}.${filters[0].extensions[0]}`, filters });
    if (path) report(exportMeeting(path));
  }

  /** Opens the report in a hidden frame and prints it; "Save as PDF" is a printer choice. */
  async function printReport() {
    const html = await reportHtml();
    const frame = document.createElement("iframe");
    frame.style.cssText = "position:fixed;width:0;height:0;border:0;visibility:hidden";
    frame.srcdoc = html;
    frame.onload = () => {
      frame.contentWindow?.focus();
      frame.contentWindow?.print();
      setTimeout(() => frame.remove(), 60_000);
    };
    document.body.appendChild(frame);
  }

  async function importRecording() {
    setView("meeting");
    const name = await importFile();
    if (name) setTitle(name);
  }

  async function openFromHistory(id: number) {
    await open(id);
    setView("meeting");
  }
  const elapsed = useElapsedSeconds(status === "recording");
  const busy = status === "stopping" || status === "importing";
  const hasMeeting = meetingId > 0 && segments.length > 0;

  return (
    <div className="flex h-full flex-col bg-zinc-50 text-zinc-900">
      <header className="flex items-center gap-3 border-b border-zinc-200 bg-white px-4 py-2.5">
        <RecordButton status={status} onStart={() => void start(title, template)} onStop={stop} />
        <input
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          disabled={status !== "idle"}
          placeholder="Judul rapat"
          aria-label="Judul rapat"
          className="min-w-0 max-w-sm flex-1 rounded-md border border-transparent px-2 py-1.5 text-sm font-medium outline-none hover:border-zinc-200 focus:border-zinc-400 disabled:bg-transparent"
        />
        {status === "recording" && (
          <span className="flex items-center gap-2 text-sm text-zinc-600">
            <span className="h-2 w-2 animate-pulse rounded-full bg-red-600" aria-hidden />
            <span className="font-mono tabular-nums">{formatClock(elapsed)}</span>
          </span>
        )}
        {status === "stopping" && <span className="text-sm text-zinc-500">Menyelesaikan…</span>}
        {status === "importing" && <span className="text-sm text-zinc-500">Mengimpor…</span>}

        <div className="ml-auto flex items-center gap-2">
          {status === "recording" && (
            <button onClick={markNow} className={BUTTON} title="Tandai momen ini">
              Tandai
            </button>
          )}
          {hasMeeting && (
            <button
              onClick={() => void runCatchUp()}
              disabled={recap === "loading"}
              title="Ringkas apa yang baru saja dibahas"
              className="rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
            >
              Susul saya
            </button>
          )}
          <nav className="flex rounded-md bg-zinc-100 p-0.5 text-sm" aria-label="Tampilan">
            <NavButton active={view === "meeting"} onClick={() => setView("meeting")}>
              Rapat
            </NavButton>
            <NavButton active={view === "history"} disabled={status !== "idle"} onClick={() => setView("history")}>
              Riwayat
            </NavButton>
          </nav>
          <Menu>
            <label className="flex flex-col gap-1 px-2 py-1.5 text-xs text-zinc-500">
              Jenis rapat
              <select
                value={template}
                onChange={(event) => changeTemplate(event.target.value)}
                disabled={busy}
                className="rounded-md border border-zinc-300 bg-white px-2 py-1.5 text-sm text-zinc-900"
              >
                {TEMPLATES.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1 px-2 py-1.5 text-xs text-zinc-500">
              Bahasa rapat
              <select
                value={language}
                onChange={(event) => {
                  setLanguage(event.target.value);
                  report(invoke("set_language", { code: event.target.value }));
                }}
                disabled={status !== "idle"}
                className="rounded-md border border-zinc-300 bg-white px-2 py-1.5 text-sm text-zinc-900"
              >
                {LANGUAGES.map(([code, name]) => (
                  <option key={code} value={code}>
                    {name}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1 px-2 py-1.5 text-xs text-zinc-500">
              Istilah dan nama (agar ejaan benar)
              <textarea
                value={glossary}
                onChange={(event) => setGlossary(event.target.value)}
                onBlur={() => report(invoke("set_glossary", { text: glossary }))}
                rows={2}
                placeholder="Kubernetes, Budi, nama produk…"
                className="resize-none rounded-md border border-zinc-300 px-2 py-1.5 text-sm text-zinc-900 outline-none focus:border-zinc-500"
              />
            </label>
            <label className="flex flex-col gap-1 px-2 py-1.5 text-xs text-zinc-500">
              Nama saya (beri tahu saat disebut)
              <input
                value={myName}
                onChange={(event) => setMyName(event.target.value)}
                placeholder="Pisahkan dengan koma"
                className="rounded-md border border-zinc-300 px-2 py-1.5 text-sm text-zinc-900 outline-none focus:border-zinc-500"
              />
            </label>
            <div className="flex flex-col gap-1 px-2 py-1.5 text-xs text-zinc-500">
              Model AI
              <div className="flex items-center gap-2">
                <ModelPicker models={modelSettings.models} apply={modelSettings.apply} />
                <button onClick={() => setView("models")} className="text-sm text-indigo-700 hover:underline">
                  Kelola
                </button>
              </div>
            </div>
            <hr className="my-1 border-zinc-200" />
            <MenuItem onClick={() => void importRecording()} disabled={status !== "idle"}>
              Impor rekaman…
            </MenuItem>
            <MenuItem onClick={() => report(printReport())} disabled={status !== "idle" || meetingId === 0}>
              Ekspor MoM sebagai PDF…
            </MenuItem>
            <MenuItem onClick={() => void exportToFile("html")} disabled={status !== "idle" || meetingId === 0}>
              Ekspor MoM sebagai HTML…
            </MenuItem>
            <MenuItem onClick={() => void exportToFile("md")} disabled={status !== "idle" || meetingId === 0}>
              Ekspor sebagai Markdown…
            </MenuItem>
            <MenuItem onClick={() => void exportToFile("raw")} disabled={status !== "idle" || meetingId === 0}>
              Ekspor transkrip / data mentah…
            </MenuItem>
            <MenuItem onClick={() => void draftEmail()} disabled={status !== "idle" || !hasMeeting || email === "loading"}>
              Tulis email tindak lanjut
            </MenuItem>
            <label className="flex cursor-pointer items-center gap-2 px-2 py-1.5 text-sm">
              <input type="checkbox" checked={autoRecord} onChange={(event) => setAutoRecord(event.target.checked)} />
              Rekam otomatis saat rapat terdeteksi
            </label>
            <p className="px-2 py-1.5 text-xs text-zinc-500">
              Pintasan global: <kbd className="rounded border border-zinc-300 bg-zinc-50 px-1">Ctrl+Alt+R</kbd> mulai atau
              berhenti merekam dari aplikasi mana pun.
            </p>
            {autostart.enabled !== null && (
              <>
                <hr className="my-1 border-zinc-200" />
                <label className="flex cursor-pointer items-center gap-2 px-2 py-1.5 text-sm">
                  <input
                    type="checkbox"
                    checked={autostart.enabled}
                    onChange={(event) => report(autostart.change(event.target.checked))}
                  />
                  Jalankan di latar belakang saat Windows mulai
                </label>
              </>
            )}
          </Menu>
        </div>
      </header>

      {detected && status === "idle" && (
        <section className="flex items-center gap-3 border-b border-indigo-200 bg-indigo-50 px-4 py-2.5 text-sm text-indigo-950">
          <p className="min-w-0 flex-1">
            <strong>{detected}</strong> terdeteksi. Rekam rapat ini?
          </p>
          <button
            onClick={() => {
              setDetected(null);
              void start(title, template);
            }}
            className="rounded-md bg-red-600 px-3 py-1 font-medium text-white hover:bg-red-700"
          >
            Rekam
          </button>
          <button onClick={() => setDetected(null)} className="text-indigo-700 hover:text-indigo-950">
            Nanti
          </button>
        </section>
      )}

      {mention && (
        <section role="alert" className="flex items-start gap-3 border-b border-amber-300 bg-amber-50 px-4 py-2.5 text-sm text-amber-950">
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

      {recap && (
        <section className="flex items-start gap-3 border-b border-indigo-200 bg-indigo-50 px-4 py-3 text-sm text-indigo-950">
          <div className="min-w-0 flex-1">
            <p className="mb-1 flex items-center gap-1 text-xs text-indigo-700">
              Susul saya ·
              {[3, 5, 10, 30].map((minutes) => (
                <button
                  key={minutes}
                  onClick={() => void runCatchUp(minutes)}
                  disabled={recap === "loading"}
                  className={`rounded px-1.5 py-0.5 ${minutes === recapMinutes ? "bg-indigo-600 text-white" : "hover:bg-indigo-100"}`}
                >
                  {minutes} menit
                </button>
              ))}
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
        <section className="flex items-start gap-3 border-b border-zinc-200 bg-white px-4 py-3 text-sm">
          <div className="min-w-0 flex-1">
            <p className="mb-1 text-xs text-zinc-500">Email tindak lanjut</p>
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
        <p role="alert" className="border-b border-red-200 bg-red-50 px-4 py-2 text-sm text-red-800">
          {error ?? notice}
        </p>
      )}

      {view === "models" ? (
        <Models models={modelSettings.models} error={modelSettings.error} apply={modelSettings.apply} />
      ) : view === "history" ? (
        <History onOpen={(id) => void openFromHistory(id)} onDeletedCurrent={clear} />
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_24rem]">
          <Transcript
            segments={segments}
            status={status}
            names={names}
            highlight={myNames}
            onRename={rename}
            onEdit={edit}
            audioFiles={audioFiles}
            onBookmark={(segment) => report(addBookmark(segment.text.slice(0, 80), segment.start_secs))}
            onStart={() => void start(title, template)}
            onImport={() => void importRecording()}
          />
          <aside className="flex min-h-0 flex-col border-l border-zinc-200 bg-white">
            <div role="tablist" className="flex gap-4 border-b border-zinc-200 px-4">
              {PANELS.map((option) => (
                <button
                  key={option.id}
                  role="tab"
                  aria-selected={panel === option.id}
                  onClick={() => setPanel(option.id)}
                  className={`-mb-px border-b-2 py-2.5 text-sm ${
                    panel === option.id ? "border-zinc-900 font-medium" : "border-transparent text-zinc-500 hover:text-zinc-900"
                  }`}
                >
                  {option.label}
                </button>
              ))}
            </div>
            {panel === "summary" && (
              <MomPanel mom={mom} pending={pending} canRetry={status === "idle"} brief={status === "recording" ? brief : undefined} />
            )}
            {panel === "ask" && (
              <AskPanel
                key={meetingId}
                enabled={meetingId > 0}
                savedChat={opened?.chat ?? []}
                suggestions={questions}
                onSuggestions={setQuestions}
              />
            )}
            {panel === "notes" && (
              <NotesPanel
                key={meetingId}
                enabled={meetingId > 0}
                savedNotes={opened?.userNotes ?? ""}
                bookmarks={bookmarks}
                onRemoveBookmark={(index) => report(removeBookmark(index))}
              />
            )}
          </aside>
        </div>
      )}
    </div>
  );
}

function NavButton(props: { active: boolean; disabled?: boolean; onClick: () => void; children: string }) {
  return (
    <button
      onClick={props.onClick}
      disabled={props.disabled}
      aria-current={props.active ? "page" : undefined}
      className={`rounded px-3 py-1 disabled:opacity-50 ${props.active ? "bg-white font-medium shadow-sm" : "text-zinc-600 hover:text-zinc-900"}`}
    >
      {props.children}
    </button>
  );
}

/** A "more" button with a panel of the things that are not needed during a meeting. Closes after a click on a button. */
function Menu({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const outside = (event: MouseEvent) => {
      if (!box.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  return (
    <div ref={box} className="relative">
      <button
        onClick={() => setOpen((shown) => !shown)}
        aria-label="Pengaturan dan lainnya"
        aria-expanded={open}
        className="rounded-md px-2.5 py-1.5 text-lg leading-none text-zinc-600 hover:bg-zinc-100"
      >
        ⋯
      </button>
      {open && (
        <div
          onClick={(event) => {
            if ((event.target as HTMLElement).closest("button")) setOpen(false);
          }}
          className="absolute right-0 top-full z-10 mt-1 flex w-80 flex-col rounded-lg border border-zinc-200 bg-white p-1.5 shadow-lg"
        >
          {children}
        </div>
      )}
    </div>
  );
}

function MenuItem(props: { onClick: () => void; disabled?: boolean; children: string }) {
  return (
    <button
      onClick={props.onClick}
      disabled={props.disabled}
      className="rounded-md px-2 py-1.5 text-left text-sm hover:bg-zinc-100 disabled:opacity-40 disabled:hover:bg-transparent"
    >
      {props.children}
    </button>
  );
}

function RecordButton(props: { status: Status; onStart: () => void; onStop: () => void }) {
  if (props.status === "idle" || props.status === "importing") {
    return (
      <button
        onClick={props.onStart}
        disabled={props.status === "importing"}
        className="rounded-full bg-red-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50"
      >
        Rekam
      </button>
    );
  }
  return (
    <button
      onClick={props.onStop}
      disabled={props.status === "stopping"}
      className="rounded-full bg-zinc-900 px-4 py-1.5 text-sm font-medium text-white hover:bg-black disabled:opacity-50"
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
  onEdit: (segment: Segment, text: string) => void;
  audioFiles: Record<string, string>;
  onStart: () => void;
  onImport: () => void;
}

/** Consecutive lines by the same speaker, close in time, read as one paragraph block. */
interface Turn {
  label: string;
  source: Source;
  start: number;
  lines: Segment[];
}

const TURN_GAP_SECS = 30;

function groupTurns(segments: Segment[], names: SpeakerNames): Turn[] {
  const turns: Turn[] = [];
  for (const segment of segments) {
    const label = speakerLabel(segment, names);
    const last = turns.at(-1);
    if (last && last.label === label && segment.start_secs - last.lines[last.lines.length - 1].end_secs < TURN_GAP_SECS) {
      last.lines.push(segment);
    } else {
      turns.push({ label, source: segment.source, start: segment.start_secs, lines: [segment] });
    }
  }
  return turns;
}

/** The text with every match of `query` marked. */
function Marked({ text, query }: { text: string; query: string }) {
  if (!query) return <>{text}</>;
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const parts = text.split(new RegExp(`(${escaped})`, "i"));
  return (
    <>
      {parts.map((part, index) =>
        part.toLowerCase() === query.toLowerCase() ? (
          <mark key={index} data-match className="rounded bg-yellow-200 px-0.5">
            {part}
          </mark>
        ) : (
          part
        ),
      )}
    </>
  );
}

function plainTranscript(turns: Turn[]): string {
  return turns
    .map((turn) => `[${formatClock(turn.start)}] ${turn.label}: ${turn.lines.map((line) => line.text).join(" ")}`)
    .join("\n\n");
}

function Transcript({ segments, status, names, highlight, onRename, onBookmark, onEdit, audioFiles, onStart, onImport }: TranscriptProps) {
  const scroller = useRef<HTMLElement>(null);
  const nearBottom = useRef(true);
  useEffect(() => {
    // Follow the live transcript, but never pull the reader away from what they scrolled back to.
    const box = scroller.current;
    if (box && nearBottom.current) box.scrollTop = box.scrollHeight;
  }, [segments.length]);

  const [query, setQuery] = useState("");
  const [current, setCurrent] = useState(0);
  const [editing, setEditing] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const player = useRef<HTMLAudioElement>(null);
  /** The line being played, so its button can stop it. */
  const [playing, setPlaying] = useState<string | null>(null);

  /** Plays the recording of one line, from a moment before it starts until it ends. */
  function play(segment: Segment, id: string) {
    const audio = player.current;
    const file = audioFiles[segment.source];
    if (!audio || !file) return;
    if (playing === id) {
      audio.pause();
      setPlaying(null);
      return;
    }
    const src = convertFileSrc(file);
    if (audio.getAttribute("src") !== src) audio.src = src;
    audio.currentTime = Math.max(0, segment.start_secs - 0.3);
    audio.ontimeupdate = () => {
      if (audio.currentTime >= segment.end_secs) {
        audio.pause();
        setPlaying(null);
      }
    };
    audio.onpause = () => setPlaying((now) => (now === id ? null : now));
    void audio.play().then(() => setPlaying(id), () => setPlaying(null));
  }
  const needle = query.trim();
  const matches = needle ? segments.filter((s) => s.text.toLowerCase().includes(needle.toLowerCase())).length : 0;

  useEffect(() => setCurrent(0), [needle]);
  useEffect(() => {
    if (!needle) return;
    const marks = scroller.current?.querySelectorAll("[data-match]");
    marks?.[current % Math.max(marks.length, 1)]?.scrollIntoView({ block: "center" });
  }, [needle, current]);

  /** What the assistant said about a line, by the line's start time. */
  const [explanations, setExplanations] = useState<Record<number, { text: string; ok: boolean }>>({});

  async function explain(segment: Segment) {
    const at = segment.start_secs;
    setExplanations((state) => ({ ...state, [at]: { text: "Menjelaskan…", ok: true } }));
    try {
      const text = await explainLine(segment.text, at);
      setExplanations((state) => ({ ...state, [at]: { text, ok: true } }));
    } catch (reason) {
      setExplanations((state) => ({ ...state, [at]: { text: String(reason), ok: false } }));
    }
  }

  if (segments.length === 0) {
    return (
      <main className="flex flex-col items-center justify-center gap-3 px-5 text-center">
        {status === "idle" ? (
          <>
            <p className="text-lg font-medium">Siap mencatat rapat Anda</p>
            <p className="max-w-sm text-sm text-zinc-500">
              Rekam mikrofon dan suara rapat sekaligus. Transkrip, ringkasan, dan tindak lanjut muncul otomatis.
            </p>
            <div className="mt-2 flex items-center gap-3">
              <button onClick={onStart} className="rounded-full bg-red-600 px-5 py-2 text-sm font-medium text-white hover:bg-red-700">
                Mulai merekam
              </button>
              <button onClick={onImport} className="text-sm text-zinc-600 hover:text-zinc-900">
                atau impor file rekaman
              </button>
            </div>
          </>
        ) : (
          <p className="text-sm text-zinc-500">Menunggu ucapan…</p>
        )}
      </main>
    );
  }

  const turns = groupTurns(segments, names);
  const key = (segment: Segment) => `${segment.source}-${segment.start_secs}`;

  return (
    <main
      ref={scroller}
      onScroll={(event) => {
        const box = event.currentTarget;
        nearBottom.current = box.scrollHeight - box.scrollTop - box.clientHeight < 80;
      }}
      className="min-h-0 overflow-y-auto"
    >
      <div className="sticky top-0 z-[1] flex items-center gap-2 border-b border-zinc-200 bg-zinc-50/95 px-5 py-2 backdrop-blur">
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && matches > 0) setCurrent((value) => (value + (event.shiftKey ? matches - 1 : 1)) % matches);
            if (event.key === "Escape") setQuery("");
          }}
          placeholder="Cari di transkrip"
          aria-label="Cari di transkrip"
          className="min-w-0 flex-1 rounded-md border border-zinc-300 bg-white px-2.5 py-1.5 text-sm outline-none focus:border-zinc-500"
        />
        {needle && (
          <span className="whitespace-nowrap text-xs text-zinc-500">
            {matches === 0 ? "Tidak ada" : `${(current % matches) + 1} dari ${matches}`}
          </span>
        )}
        <button
          onClick={() =>
            void navigator.clipboard.writeText(plainTranscript(turns)).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            })
          }
          className="whitespace-nowrap rounded-md border border-zinc-300 bg-white px-2.5 py-1.5 text-sm hover:bg-zinc-100"
        >
          {copied ? "Tersalin" : "Salin"}
        </button>
      </div>
      <audio ref={player} preload="none" />
      <div className="px-5 py-4">
        {status === "idle" && <SpeakerNamesForm segments={segments} names={names} onRename={onRename} />}
        <ol className="mx-auto flex max-w-3xl flex-col gap-5">
          {turns.map((turn) => (
            <li key={`${turn.source}-${turn.start}`} className="text-sm">
              <p className="mb-1 flex items-baseline gap-2">
                <span className={`font-semibold ${SPEAKER_COLOR[turn.source]}`}>{turn.label}</span>
                <time className="font-mono text-xs tabular-nums text-zinc-400">{formatClock(turn.start)}</time>
              </p>
              <div className="flex flex-col gap-1">
                {turn.lines.map((segment) => (
                  <div key={key(segment)} className="group flex items-start gap-2">
                    <div className="min-w-0 flex-1">
                      {editing === key(segment) ? (
                        <textarea
                          autoFocus
                          defaultValue={segment.text}
                          rows={Math.max(2, Math.ceil(segment.text.length / 70))}
                          aria-label="Perbaiki teks"
                          onKeyDown={(event) => {
                            if (event.key === "Escape") setEditing(null);
                            if (event.key === "Enter" && !event.shiftKey) {
                              event.preventDefault();
                              onEdit(segment, event.currentTarget.value);
                              setEditing(null);
                            }
                          }}
                          onBlur={(event) => {
                            onEdit(segment, event.currentTarget.value);
                            setEditing(null);
                          }}
                          className="w-full rounded-md border border-indigo-400 bg-white px-2 py-1 leading-relaxed outline-none"
                        />
                      ) : (
                        <p
                          onDoubleClick={() => status === "idle" && setEditing(key(segment))}
                          title={status === "idle" ? "Klik dua kali untuk memperbaiki" : undefined}
                          className={`leading-relaxed ${
                            segment.source === "system" && mentions(segment.text, highlight) ? "rounded bg-amber-100 px-1" : ""
                          }`}
                        >
                          <Marked text={segment.text} query={needle} />
                        </p>
                      )}
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
                    <span
                      className={`flex items-start gap-2 focus-within:opacity-100 group-hover:opacity-100 ${
                        playing === key(segment) ? "" : "opacity-0"
                      }`}
                    >
                      {status === "idle" && audioFiles[segment.source] && (
                        <button
                          onClick={() => play(segment, key(segment))}
                          aria-label={`${playing === key(segment) ? "Hentikan" : "Putar"} ${formatClock(segment.start_secs)}`}
                          title="Dengarkan bagian ini"
                          className="text-zinc-400 hover:text-zinc-900"
                        >
                          {playing === key(segment) ? "■" : "▶"}
                        </button>
                      )}
                      {status === "idle" && (
                        <button
                          onClick={() => setEditing(key(segment))}
                          aria-label={`Perbaiki ${formatClock(segment.start_secs)}`}
                          title="Perbaiki teks"
                          className="text-zinc-400 hover:text-zinc-900"
                        >
                          ✎
                        </button>
                      )}
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
                  </div>
                ))}
              </div>
            </li>
          ))}
        </ol>
      </div>
    </main>
  );
}

/** Shown once the final diarization has numbered the speakers. */
function SpeakerNamesForm({
  segments,
  names,
  onRename,
}: Omit<TranscriptProps, "status" | "highlight" | "onBookmark" | "onEdit" | "audioFiles" | "onStart" | "onImport">) {
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
