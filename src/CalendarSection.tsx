import { useEffect, useState } from "react";
import { getCalendar, setCalendar } from "./notes";

const field = "min-w-0 flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm outline-none focus:border-zinc-500";

/** The calendar feed (.ics): names a meeting after the entry that is on, and suggests the invited people as speaker names. */
export function CalendarSection() {
  const [source, setSource] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getCalendar().then(setSource, () => {});
  }, []);

  async function save() {
    setBusy(true);
    setMessage(null);
    try {
      const count = await setCalendar(source);
      setMessage(source.trim() ? `Tersimpan. ${count} jadwal terbaca.` : "Kalender dimatikan.");
    } catch (reason) {
      setMessage(String(reason));
    }
    setBusy(false);
  }

  return (
    <section className="flex flex-col gap-3 rounded-lg border border-zinc-200 p-4">
      <div>
        <h3 className="text-sm font-medium">Kalender</h3>
        <p className="text-xs text-zinc-500">
          Tempel alamat iCal (.ics) kalender Anda: di Google Calendar, "Alamat rahasia dalam format iCal" pada pengaturan
          kalender; di Outlook, kalender yang dipublikasikan. Rapat tanpa judul lalu memakai nama jadwal yang sedang berjalan,
          dan peserta undangan muncul sebagai saran nama pembicara. Tanpa login; alamat ini disimpan hanya di komputer ini.
        </p>
      </div>
      <div className="flex gap-2">
        <input
          value={source}
          onChange={(event) => setSource(event.target.value)}
          placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"
          aria-label="Alamat kalender"
          className={field}
        />
        <button
          onClick={() => void save()}
          disabled={busy}
          className="rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-900 disabled:opacity-50"
        >
          {busy ? "Membaca…" : "Simpan"}
        </button>
      </div>
      {message && (
        <p role="status" className="text-xs text-zinc-600">
          {message}
        </p>
      )}
    </section>
  );
}
