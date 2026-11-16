import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { type FormEvent, useEffect, useState } from "react";
import { type LibraryAnswer, askLibrary, deleteMeeting } from "./notes";

interface MeetingSummary {
  id: number;
  title: string;
  /** Seconds since the Unix epoch. */
  started_at: number;
  duration_secs: number;
}

const DATE_FORMAT = new Intl.DateTimeFormat("id-ID", { dateStyle: "medium", timeStyle: "short" });

interface Props {
  onOpen: (id: number) => void;
  /** The meeting that is open behind the history was deleted. */
  onDeletedCurrent: () => void;
}

/** The saved meetings, newest first, filtered by title and transcript text, and questions across all of them. */
export function History({ onOpen, onDeletedCurrent }: Props) {
  const [query, setQuery] = useState("");
  const [meetings, setMeetings] = useState<MeetingSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Bumped after a deletion to list again. */
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let current = true;
    invoke<MeetingSummary[]>("list_meetings", { query }).then(
      (found) => current && setMeetings(found),
      (reason) => current && setError(String(reason)),
    );
    return () => {
      current = false;
    };
  }, [query, version]);

  async function remove(meeting: MeetingSummary) {
    const sure = await confirm(`Hapus "${meeting.title}" beserta rekaman audionya? Ini tidak bisa dibatalkan.`, {
      title: "Hapus rapat",
      kind: "warning",
    });
    if (!sure) return;
    try {
      if (await deleteMeeting(meeting.id)) onDeletedCurrent();
      setError(null);
      setVersion((current) => current + 1);
    } catch (reason) {
      setError(String(reason));
    }
  }

  return (
    <main className="min-h-0 flex-1 overflow-y-auto bg-white px-5 py-4">
      <div className="mx-auto flex max-w-3xl flex-col gap-3">
        <AskLibrary onOpen={onOpen} />
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Cari di judul dan transkrip"
          autoFocus
          className="rounded-md border border-zinc-300 px-3 py-2 text-sm outline-none focus:border-zinc-500"
        />
        {error && <p className="text-sm text-red-700">{error}</p>}
        {meetings?.length === 0 && (
          <p className="text-sm text-zinc-500">{query.trim() ? "Tidak ada rapat yang cocok." : "Belum ada rapat tersimpan."}</p>
        )}
        <ul className="divide-y divide-zinc-200">
          {meetings?.map((meeting) => (
            <li key={meeting.id} className="flex items-center hover:bg-zinc-50">
              <button
                onClick={() => onOpen(meeting.id)}
                className="flex min-w-0 flex-1 items-baseline justify-between gap-4 px-2 py-3 text-left text-sm"
              >
                <span className="truncate font-medium">{meeting.title}</span>
                <span className="shrink-0 text-zinc-500">
                  {DATE_FORMAT.format(meeting.started_at * 1000)} · {Math.max(1, Math.round(meeting.duration_secs / 60))} menit
                </span>
              </button>
              <button
                onClick={() => void remove(meeting)}
                aria-label={`Hapus ${meeting.title}`}
                className="px-3 py-2 text-sm text-zinc-400 hover:text-red-700"
              >
                Hapus
              </button>
            </li>
          ))}
        </ul>
      </div>
    </main>
  );
}

/** A question to all saved meetings, answered from their minutes and transcripts. */
function AskLibrary({ onOpen }: { onOpen: (id: number) => void }) {
  const [draft, setDraft] = useState("");
  const [asked, setAsked] = useState<string | null>(null);
  const [result, setResult] = useState<LibraryAnswer | { error: string } | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const question = draft.trim();
    if (!question || (asked && !result)) return;
    setAsked(question);
    setResult(null);
    setResult(await askLibrary(question).catch((reason) => ({ error: String(reason) })));
  }

  return (
    <section className="flex flex-col gap-2 rounded-md border border-zinc-200 bg-zinc-50 p-3">
      <form onSubmit={(event) => void submit(event)} className="flex gap-2">
        <input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="Tanya semua rapat, mis. apa keputusan soal anggaran?"
          aria-label="Tanya semua rapat"
          className="min-w-0 flex-1 rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm outline-none focus:border-zinc-500"
        />
        <button
          disabled={!draft.trim() || (asked !== null && result === null)}
          className="rounded-md bg-zinc-800 px-3 py-2 text-sm font-medium text-white hover:bg-zinc-900 disabled:opacity-50"
        >
          Tanya
        </button>
      </form>
      {asked && (
        <div className="flex flex-col gap-2 text-sm">
          <p className="font-medium">{asked}</p>
          {result === null && <p className="text-zinc-500">Mencari di semua rapat…</p>}
          {result && "error" in result && <p className="text-red-700">{result.error}</p>}
          {result && "answer" in result && (
            <>
              <p className="whitespace-pre-wrap leading-relaxed text-zinc-700">{result.answer}</p>
              {result.sources.length > 0 && (
                <p className="flex flex-wrap items-center gap-2 text-xs text-zinc-500">
                  Sumber:
                  {result.sources.map((source) => (
                    <button
                      key={source.meeting_id}
                      onClick={() => onOpen(source.meeting_id)}
                      className="rounded-md border border-zinc-300 bg-white px-2 py-0.5 hover:bg-zinc-100"
                    >
                      {source.title} · {DATE_FORMAT.format(source.started_at * 1000)}
                    </button>
                  ))}
                </p>
              )}
            </>
          )}
        </div>
      )}
    </section>
  );
}
