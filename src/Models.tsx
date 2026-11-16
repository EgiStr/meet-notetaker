import { type FormEvent, useState } from "react";
import {
  type ModelsView,
  type Provider,
  activateModel,
  addModel,
  fetchModels,
  removeModel,
  removeProvider,
  saveProvider,
  testModel,
} from "./providers";

const PRESETS = [
  { name: "Gemini (Google)", url: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-2.5-flash" },
  { name: "9router (lokal)", url: "http://localhost:20128/v1", model: "" },
  { name: "OpenAI", url: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  { name: "Groq", url: "https://api.groq.com/openai/v1", model: "llama-3.3-70b-versatile" },
  { name: "OpenRouter", url: "https://openrouter.ai/api/v1", model: "" },
  { name: "Ollama (lokal)", url: "http://localhost:11434/v1", model: "" },
];

interface Props {
  models: ModelsView | null;
  error: string | null;
  apply: (change: Promise<ModelsView>) => Promise<void>;
}

/** Providers and models: add, remove, test, and choose the one the app uses. */
export function Models({ models, error, apply }: Props) {
  return (
    <main className="min-h-0 flex-1 overflow-y-auto bg-white px-5 py-4">
      <div className="mx-auto flex max-w-3xl flex-col gap-4">
        <p className="text-sm text-zinc-600">
          Pakai penyedia mana pun yang kompatibel dengan OpenAI: layanan awan, gateway seperti 9router, atau server lokal
          seperti Ollama. Kunci API disimpan di Windows Credential Manager, bukan di berkas.
        </p>
        {models?.env_fallback && (
          <p className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-900">
            Belum ada model dipilih; aplikasi memakai variabel lingkungan LLM_*. Tambahkan model di bawah untuk menggantinya.
          </p>
        )}
        {error && <p className="text-sm text-red-700">{error}</p>}
        {models?.providers.map((provider) => (
          <ProviderCard key={provider.id} provider={provider} active={models.active} apply={apply} />
        ))}
        <NewProvider apply={apply} />
      </div>
    </main>
  );
}

function ProviderCard({ provider, active, apply }: { provider: Provider; active: ModelsView["active"]; apply: Props["apply"] }) {
  const [model, setModel] = useState("");
  const [available, setAvailable] = useState<string[] | null>(null);
  const [key, setKey] = useState("");
  /** The outcome of the last test or listing, per model or for the whole provider. */
  const [message, setMessage] = useState<{ text: string; ok: boolean } | null>(null);
  const [busy, setBusy] = useState(false);

  async function run(work: () => Promise<string>) {
    setBusy(true);
    setMessage(null);
    try {
      setMessage({ text: await work(), ok: true });
    } catch (reason) {
      setMessage({ text: String(reason), ok: false });
    } finally {
      setBusy(false);
    }
  }

  const listModels = () =>
    run(async () => {
      const found = await fetchModels(provider.id);
      setAvailable(found);
      return `${found.length} model tersedia.`;
    });

  function add(event: FormEvent) {
    event.preventDefault();
    if (!model.trim()) return;
    void apply(addModel(provider.id, model));
    setModel("");
  }

  const filtered = (available ?? []).filter((id) => id.toLowerCase().includes(model.trim().toLowerCase())).slice(0, 12);

  return (
    <section className="flex flex-col gap-3 rounded-md border border-zinc-200 p-4">
      <div className="flex items-baseline gap-3">
        <h2 className="font-medium">{provider.name}</h2>
        <span className="truncate text-xs text-zinc-500">{provider.base_url}</span>
        <span className={`text-xs ${provider.has_key ? "text-emerald-700" : "text-zinc-500"}`}>
          {provider.has_key ? "kunci tersimpan" : "tanpa kunci"}
        </span>
        <button
          onClick={() => void apply(removeProvider(provider.id))}
          className="ml-auto text-sm text-zinc-400 hover:text-red-700"
        >
          Hapus penyedia
        </button>
      </div>

      <ul className="flex flex-col divide-y divide-zinc-100">
        {provider.models.map((id) => {
          const isActive = active?.provider === provider.id && active.model === id;
          return (
            <li key={id} className="flex items-center gap-2 py-1.5 text-sm">
              <span className="min-w-0 flex-1 truncate font-mono">{id}</span>
              {isActive ? (
                <span className="rounded-md bg-zinc-800 px-2 py-0.5 text-xs text-white">dipakai</span>
              ) : (
                <button
                  onClick={() => void apply(activateModel(provider.id, id))}
                  className="rounded-md border border-zinc-300 px-2 py-0.5 text-xs hover:bg-zinc-50"
                >
                  Pakai
                </button>
              )}
              <button
                disabled={busy}
                onClick={() => void run(async () => `${id}: ${await testModel(provider.id, id)}`)}
                className="rounded-md border border-zinc-300 px-2 py-0.5 text-xs hover:bg-zinc-50 disabled:opacity-50"
              >
                Uji
              </button>
              <button
                onClick={() => void apply(removeModel(provider.id, id))}
                aria-label={`Hapus ${id}`}
                className="text-xs text-zinc-400 hover:text-red-700"
              >
                Hapus
              </button>
            </li>
          );
        })}
        {provider.models.length === 0 && <li className="py-1.5 text-sm text-zinc-500">Belum ada model. Tambahkan di bawah.</li>}
      </ul>

      <form onSubmit={add} className="flex flex-col gap-2">
        <div className="flex gap-2">
          <input
            value={model}
            onChange={(event) => setModel(event.target.value)}
            placeholder="ID model, mis. gemini-2.5-flash"
            aria-label={`Model baru untuk ${provider.name}`}
            className="min-w-0 flex-1 rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-sm outline-none focus:border-zinc-500"
          />
          <button
            disabled={!model.trim()}
            className="rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-900 disabled:opacity-50"
          >
            Tambah
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void listModels()}
            className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
          >
            Ambil daftar
          </button>
        </div>
        {available && filtered.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {filtered.map((id) => (
              <button
                type="button"
                key={id}
                onClick={() => setModel(id)}
                className="rounded-md border border-zinc-200 bg-zinc-50 px-2 py-0.5 font-mono text-xs hover:bg-zinc-100"
              >
                {id}
              </button>
            ))}
          </div>
        )}
      </form>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!key.trim()) return;
          void apply(saveProvider(provider.id, provider.name, provider.base_url, key));
          setKey("");
        }}
        className="flex gap-2"
      >
        <input
          type="password"
          value={key}
          onChange={(event) => setKey(event.target.value)}
          placeholder={provider.has_key ? "Ganti kunci API" : "Kunci API"}
          aria-label={`Kunci API ${provider.name}`}
          autoComplete="off"
          className="min-w-0 flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm outline-none focus:border-zinc-500"
        />
        <button
          disabled={!key.trim()}
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-50 disabled:opacity-50"
        >
          Simpan kunci
        </button>
      </form>

      {message && <p className={`text-sm ${message.ok ? "text-emerald-700" : "text-red-700"}`}>{message.text}</p>}
    </section>
  );
}

function NewProvider({ apply }: { apply: Props["apply"] }) {
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [key, setKey] = useState("");

  function submit(event: FormEvent) {
    event.preventDefault();
    void apply(saveProvider(null, name, url, key));
    setName("");
    setUrl("");
    setKey("");
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-2 rounded-md border border-dashed border-zinc-300 p-4">
      <h2 className="font-medium">Tambah penyedia</h2>
      <div className="flex flex-wrap gap-1.5">
        {PRESETS.map((preset) => (
          <button
            type="button"
            key={preset.name}
            onClick={() => {
              setName(preset.name);
              setUrl(preset.url);
            }}
            className="rounded-md border border-zinc-200 bg-zinc-50 px-2 py-0.5 text-xs hover:bg-zinc-100"
          >
            {preset.name}
          </button>
        ))}
      </div>
      <input
        value={name}
        onChange={(event) => setName(event.target.value)}
        placeholder="Nama"
        aria-label="Nama penyedia"
        className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm outline-none focus:border-zinc-500"
      />
      <input
        value={url}
        onChange={(event) => setUrl(event.target.value)}
        placeholder="Alamat dasar, mis. http://localhost:20128/v1"
        aria-label="Alamat penyedia"
        className="rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-sm outline-none focus:border-zinc-500"
      />
      <input
        type="password"
        value={key}
        onChange={(event) => setKey(event.target.value)}
        placeholder="Kunci API (boleh kosong untuk server lokal)"
        aria-label="Kunci API penyedia baru"
        autoComplete="off"
        className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm outline-none focus:border-zinc-500"
      />
      <button
        disabled={!name.trim() || !url.trim()}
        className="self-start rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-900 disabled:opacity-50"
      >
        Tambah penyedia
      </button>
    </form>
  );
}

/** A quick switch in the header; the Model screen adds and removes. */
export function ModelPicker({ models, apply }: Pick<Props, "models" | "apply">) {
  const options = models?.providers.flatMap((provider) => provider.models.map((id) => ({ provider, id }))) ?? [];
  if (options.length === 0) return null;
  const value = models?.active ? `${models.active.provider}\u0000${models.active.model}` : "";
  return (
    <select
      value={value}
      onChange={(event) => {
        const [provider, model] = event.target.value.split("\u0000");
        void apply(activateModel(provider, model));
      }}
      aria-label="Model AI"
      className="max-w-48 rounded-md border border-zinc-300 px-2 py-1.5 text-sm"
    >
      {!value && <option value="">Pilih model…</option>}
      {options.map(({ provider, id }) => (
        <option key={`${provider.id}\u0000${id}`} value={`${provider.id}\u0000${id}`}>
          {id}
        </option>
      ))}
    </select>
  );
}
