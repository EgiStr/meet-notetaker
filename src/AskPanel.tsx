import { type FormEvent, useState } from "react";
import { ask, setUserNotes } from "./notes";

interface Exchange {
  question: string;
  /** Null while the answer is being written. */
  answer: string | null;
  failed?: boolean;
}

interface Props {
  enabled: boolean;
  /** What a meeting opened from the history already has. */
  savedNotes: string;
  savedChat: Exchange[];
}

/** The user's own notes and on-demand questions about the meeting. Remount (change `key`) to show another meeting. */
export function AskPanel({ enabled, savedNotes, savedChat }: Props) {
  return (
    <section className="flex min-h-0 flex-col bg-white">
      <UserNotes enabled={enabled} saved={savedNotes} />
      <Questions enabled={enabled} saved={savedChat} />
    </section>
  );
}

function UserNotes({ enabled, saved }: { enabled: boolean; saved: string }) {
  const [text, setText] = useState(saved);
  const [error, setError] = useState<string | null>(null);

  function change(next: string) {
    setText(next);
    setUserNotes(next).then(
      () => setError(null),
      (reason) => setError(String(reason)),
    );
  }

  return (
    <div className="flex flex-col border-b border-zinc-200">
      <PanelTitle>Catatan saya</PanelTitle>
      <textarea
        value={text}
        onChange={(event) => change(event.target.value)}
        disabled={!enabled}
        rows={5}
        placeholder={enabled ? "Hal yang penting bagi Anda. Ikut dipakai saat MoM disusun." : "Tersedia setelah rekaman dimulai."}
        className="resize-none px-4 py-3 text-sm outline-none placeholder:text-zinc-400 disabled:bg-zinc-50"
      />
      {error && <p className="px-4 pb-2 text-xs text-red-700">{error}</p>}
    </div>
  );
}

function Questions({ enabled, saved }: { enabled: boolean; saved: Exchange[] }) {
  const [exchanges, setExchanges] = useState<Exchange[]>(saved);
  const [draft, setDraft] = useState("");
  const waiting = exchanges.some((exchange) => exchange.answer === null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const question = draft.trim();
    if (!question || waiting) return;
    setDraft("");
    setExchanges((current) => [...current, { question, answer: null }]);
    const result = await ask(question).then(
      (answer) => ({ answer }),
      (reason) => ({ answer: String(reason), failed: true }),
    );
    setExchanges((current) => current.map((exchange) => (exchange.answer === null ? { question, ...result } : exchange)));
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PanelTitle>Tanya</PanelTitle>
      <ol className="flex flex-1 flex-col gap-3 overflow-y-auto px-4 py-3 text-sm">
        {exchanges.length === 0 && <li className="text-zinc-500">Tanyakan apa saja tentang isi rapat ini.</li>}
        {exchanges.map((exchange, index) => (
          <li key={index}>
            <p className="font-medium">{exchange.question}</p>
            <p className={`whitespace-pre-wrap leading-relaxed ${exchange.failed ? "text-red-700" : "text-zinc-700"}`}>
              {exchange.answer ?? "Menjawab…"}
            </p>
          </li>
        ))}
      </ol>
      <form onSubmit={submit} className="flex gap-2 border-t border-zinc-200 p-3">
        <input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          disabled={!enabled}
          placeholder="Apa keputusan soal…?"
          className="min-w-0 flex-1 rounded-md border border-zinc-300 px-3 py-1.5 text-sm outline-none focus:border-zinc-500 disabled:bg-zinc-50"
        />
        <button
          disabled={!enabled || waiting || !draft.trim()}
          className="rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-900 disabled:opacity-50"
        >
          Tanya
        </button>
      </form>
    </div>
  );
}

function PanelTitle({ children }: { children: string }) {
  return (
    <h2 className="border-b border-zinc-200 px-4 py-2 text-xs font-semibold uppercase tracking-wide text-zinc-500">
      {children}
    </h2>
  );
}
