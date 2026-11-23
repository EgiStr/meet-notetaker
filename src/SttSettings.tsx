import { type FormEvent, useEffect, useState } from "react";
import { type SttSettings, type SttView, getStt, resetStt, saveStt, testStt } from "./providers";

const PRESETS: { name: string; settings: SttSettings }[] = [
  {
    name: "Groq (Whisper)",
    settings: { provider: "openai", base_url: "https://api.groq.com/openai/v1", model: "whisper-large-v3-turbo", final_model: "whisper-large-v3" },
  },
  {
    name: "OpenAI",
    settings: { provider: "openai", base_url: "https://api.openai.com/v1", model: "whisper-1", final_model: "" },
  },
  { name: "Deepgram", settings: { provider: "deepgram", base_url: "", model: "nova-2", final_model: "" } },
];

const field = "rounded-md border border-zinc-300 px-3 py-1.5 text-sm outline-none focus:border-zinc-500";

/** The transcription service: provider, models and key, kept in the app instead of environment variables. */
export function SttSection() {
  const [view, setView] = useState<SttView | null>(null);
  const [form, setForm] = useState<SttSettings | null>(null);
  const [key, setKey] = useState("");
  const [message, setMessage] = useState<{ text: string; ok: boolean } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getStt().then(
      (loaded) => {
        setView(loaded);
        setForm(loaded.settings);
      },
      (reason) => setMessage({ text: String(reason), ok: false }),
    );
  }, []);

  async function run(work: () => Promise<string | SttView>) {
    setBusy(true);
    setMessage(null);
    try {
      const result = await work();
      if (typeof result === "string") {
        setMessage({ text: result, ok: true });
      } else {
        setView(result);
        setForm(result.settings);
        setKey("");
        setMessage({ text: "Tersimpan.", ok: true });
      }
    } catch (reason) {
      setMessage({ text: String(reason), ok: false });
    } finally {
      setBusy(false);
    }
  }

  if (!form) return message && <p className="text-sm text-red-700">{message.text}</p>;
  const set = (change: Partial<SttSettings>) => setForm({ ...form, ...change });
  const openai = form.provider === "openai";

  function submit(event: FormEvent) {
    event.preventDefault();
    if (form) void run(() => saveStt(form, key));
  }

  return (
    <section className="flex flex-col gap-3 rounded-md border border-zinc-200 p-4">
      <div className="flex items-baseline gap-3">
        <h2 className="font-medium">Transkripsi (suara ke teks)</h2>
        <span className={`text-xs ${view?.has_key ? "text-emerald-700" : "text-zinc-500"}`}>
          {view?.has_key ? "kunci tersimpan" : "belum ada kunci"}
        </span>
        {!view?.env_fallback && (
          <button disabled={busy} onClick={() => void run(resetStt)} className="ml-auto text-sm text-zinc-400 hover:text-red-700">
            Pakai variabel lingkungan
          </button>
        )}
      </div>
      {view?.env_fallback && (
        <p className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-900">
          Belum diatur di sini; aplikasi memakai variabel lingkungan STT_* bila ada. Isi di bawah agar aplikasi yang
          terpasang bisa mentranskripsi tanpa variabel lingkungan.
        </p>
      )}
      <div className="flex flex-wrap gap-1.5">
        {PRESETS.map((preset) => (
          <button
            type="button"
            key={preset.name}
            onClick={() => setForm(preset.settings)}
            className="rounded-md border border-zinc-200 bg-zinc-50 px-2 py-0.5 text-xs hover:bg-zinc-100"
          >
            {preset.name}
          </button>
        ))}
      </div>
      <form onSubmit={submit} className="flex flex-col gap-2">
        <select value={form.provider} onChange={(event) => set({ provider: event.target.value as SttSettings["provider"] })} aria-label="Penyedia transkripsi" className={field}>
          <option value="openai">Kompatibel OpenAI (Groq, OpenAI, server Whisper lokal)</option>
          <option value="deepgram">Deepgram</option>
        </select>
        {openai && (
          <input value={form.base_url} onChange={(event) => set({ base_url: event.target.value })} placeholder="Alamat dasar" aria-label="Alamat layanan transkripsi" className={`${field} font-mono`} />
        )}
        <input value={form.model} onChange={(event) => set({ model: event.target.value })} placeholder="Model saat merekam (cepat)" aria-label="Model transkripsi langsung" className={`${field} font-mono`} />
        {openai && (
          <input value={form.final_model} onChange={(event) => set({ final_model: event.target.value })} placeholder="Model untuk transkrip akhir (kosong = otomatis)" aria-label="Model transkrip akhir" className={`${field} font-mono`} />
        )}
        <input type="password" value={key} onChange={(event) => setKey(event.target.value)} placeholder={view?.has_key ? "Ganti kunci API" : "Kunci API"} aria-label="Kunci API transkripsi" autoComplete="off" className={field} />
        <div className="flex gap-2">
          <button disabled={busy || (!view?.has_key && !key.trim())} className="rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-900 disabled:opacity-50">
            Simpan
          </button>
          {openai && !view?.env_fallback && (
            <button type="button" disabled={busy} onClick={() => void run(testStt)} className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50">
              Uji koneksi
            </button>
          )}
        </div>
      </form>
      {message && <p className={`text-sm ${message.ok ? "text-emerald-700" : "text-red-700"}`}>{message.text}</p>}
    </section>
  );
}
