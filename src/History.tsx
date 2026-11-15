import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

interface MeetingSummary {
  id: number;
  title: string;
  /** Seconds since the Unix epoch. */
  started_at: number;
  duration_secs: number;
}

const DATE_FORMAT = new Intl.DateTimeFormat("id-ID", { dateStyle: "medium", timeStyle: "short" });

/** The saved meetings, newest first, filtered by title and transcript text. */
export function History({ onOpen }: { onOpen: (id: number) => void }) {
  const [query, setQuery] = useState("");
  const [meetings, setMeetings] = useState<MeetingSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    invoke<MeetingSummary[]>("list_meetings", { query }).then(
      (found) => current && setMeetings(found),
      (reason) => current && setError(String(reason)),
    );
    return () => {
      current = false;
    };
  }, [query]);

  return (
    <main className="min-h-0 flex-1 overflow-y-auto bg-white px-5 py-4">
      <div className="mx-auto flex max-w-3xl flex-col gap-3">
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
            <li key={meeting.id}>
              <button
                onClick={() => onOpen(meeting.id)}
                className="flex w-full items-baseline justify-between gap-4 px-2 py-3 text-left text-sm hover:bg-zinc-50"
              >
                <span className="font-medium">{meeting.title}</span>
                <span className="shrink-0 text-zinc-500">
                  {DATE_FORMAT.format(meeting.started_at * 1000)} · {Math.max(1, Math.round(meeting.duration_secs / 60))} menit
                </span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </main>
  );
}
