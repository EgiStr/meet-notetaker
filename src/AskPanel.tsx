import { type FormEvent, useState } from "react";
import { type Bookmark, ask, expandNotes, setUserNotes, suggestQuestions } from "./notes";
import { formatClock } from "./recording";
import { TalkStats } from "./TalkStats";

interface Exchange {
  question: string;
  /** Null while the answer is being written. */
  answer: string | null;
  failed?: boolean;
}

interface NotesProps {
  enabled: boolean;
  /** What a meeting opened from the history already has. */
  savedNotes: string;
  bookmarks: Bookmark[];
  onRemoveBookmark: (index: number) => void;
  onClipBookmark: (index: number) => void;
}

/** The user's own notes and the bookmarks. Remount (change `key`) to show another meeting. */
export function NotesPanel(props: NotesProps) {
  return (
    <section className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <UserNotes enabled={props.enabled} saved={props.savedNotes} />
      <Bookmarks bookmarks={props.bookmarks} onRemove={props.onRemoveBookmark} onClip={props.onClipBookmark} />
      <TalkStats enabled={props.enabled} />
    </section>
  );
}

interface AskProps {
  enabled: boolean;
  savedChat: Exchange[];
  /** Questions worth asking, refreshed by the core while recording. */
  suggestions: string[];
  onSuggestions: (questions: string[]) => void;
}

/** On-demand questions about the meeting. Remount (change `key`) to show another meeting. */
export function AskPanel(props: AskProps) {
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <Questions enabled={props.enabled} saved={props.savedChat} suggestions={props.suggestions} onSuggestions={props.onSuggestions} />
    </section>
  );
}

function UserNotes({ enabled, saved }: { enabled: boolean; saved: string }) {
  const [text, setText] = useState(saved);
  const [error, setError] = useState<string | null>(null);
  const [expanding, setExpanding] = useState(false);
  /** The user's notes written out by the AI, until the user takes or dismisses them. */
  const [expanded, setExpanded] = useState<string | null>(null);

  function change(next: string) {
    setText(next);
    setUserNotes(next).then(
      () => setError(null),
      (reason) => setError(String(reason)),
    );
  }

  async function expand() {
    setExpanding(true);
    setError(null);
    try {
      setExpanded(await expandNotes());
    } catch (reason) {
      setError(String(reason));
    }
    setExpanding(false);
  }

  return (
    <div className="flex flex-col">
      <textarea
        value={text}
        onChange={(event) => change(event.target.value)}
        disabled={!enabled}
        rows={8}
        placeholder={enabled ? "Catatan pribadi. Ikut dipakai saat ringkasan disusun." : "Tersedia setelah rekaman dimulai."}
        className="resize-none px-4 py-3 text-sm outline-none placeholder:text-zinc-400 disabled:bg-zinc-50"
      />
      <div className="px-4 pb-2">
        <button
          onClick={() => void expand()}
          disabled={!enabled || expanding || !text.trim()}
          className="rounded-md border border-zinc-300 px-2 py-1 text-xs hover:bg-zinc-50 disabled:opacity-50"
        >
          {expanding ? "Memperluas…" : "Perluas dengan AI"}
        </button>
      </div>
      {error && <p className="px-4 pb-2 text-xs text-red-700">{error}</p>}
      {expanded && (
        <div className="mx-4 mb-3 flex flex-col gap-2 rounded-md border border-zinc-200 bg-zinc-50 p-3 text-sm">
          <p className="max-h-48 overflow-y-auto whitespace-pre-wrap leading-relaxed text-zinc-700">{expanded}</p>
          <div className="flex gap-2">
            <button
              onClick={() => {
                change(expanded);
                setExpanded(null);
              }}
              className="rounded-md bg-zinc-800 px-2 py-1 text-xs font-medium text-white hover:bg-zinc-900"
            >
              Ganti catatan saya
            </button>
            <button
              onClick={() => setExpanded(null)}
              className="rounded-md border border-zinc-300 px-2 py-1 text-xs hover:bg-white"
            >
              Tutup
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function Bookmarks({
  bookmarks,
  onRemove,
  onClip,
}: {
  bookmarks: Bookmark[];
  onRemove: (index: number) => void;
  onClip: (index: number) => void;
}) {
  if (bookmarks.length === 0) return null;
  return (
    <div className="border-t border-zinc-200">
      <h3 className="px-4 pt-3 text-xs font-medium text-zinc-500">Penanda</h3>
      <ul className="max-h-28 overflow-y-auto px-4 py-2 text-sm">
        {bookmarks.map((bookmark, index) => (
          <li key={`${bookmark.at_secs}-${index}`} className="flex items-baseline gap-2">
            <time className="font-mono tabular-nums text-zinc-500">{formatClock(bookmark.at_secs)}</time>
            <span className="min-w-0 flex-1 truncate">{bookmark.label}</span>
            <button
              onClick={() => onClip(index)}
              title="Simpan klip (audio dan teks) di sekitar penanda ini"
              className="text-xs text-zinc-500 underline hover:text-zinc-800"
            >
              Klip
            </button>
            <button
              onClick={() => onRemove(index)}
              aria-label={`Hapus penanda ${bookmark.label}`}
              className="text-zinc-400 hover:text-zinc-700"
            >
              ✕
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

interface QuestionsProps {
  enabled: boolean;
  saved: Exchange[];
  suggestions: string[];
  onSuggestions: (questions: string[]) => void;
}

function Questions({ enabled, saved, suggestions, onSuggestions }: QuestionsProps) {
  const [exchanges, setExchanges] = useState<Exchange[]>(saved);
  const [draft, setDraft] = useState("");
  const [suggesting, setSuggesting] = useState(false);
  const waiting = exchanges.some((exchange) => exchange.answer === null);

  async function submit(question: string) {
    if (!question || waiting) return;
    setExchanges((current) => [...current, { question, answer: null }]);
    const result = await ask(question).then(
      (answer) => ({ answer }),
      (reason) => ({ answer: String(reason), failed: true }),
    );
    setExchanges((current) => current.map((exchange) => (exchange.answer === null ? { question, ...result } : exchange)));
  }

  function submitDraft(event: FormEvent) {
    event.preventDefault();
    const question = draft.trim();
    setDraft("");
    void submit(question);
  }

  async function suggest() {
    setSuggesting(true);
    try {
      onSuggestions(await suggestQuestions());
    } catch {
      // The suggestions are optional; a failing LLM is already shown by the minutes.
    }
    setSuggesting(false);
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ol className="flex flex-1 flex-col gap-3 overflow-y-auto px-4 py-3 text-sm">
        {exchanges.length === 0 && <li className="text-zinc-500">Tanyakan apa saja tentang rapat ini.</li>}
        {exchanges.map((exchange, index) => (
          <li key={index}>
            <p className="font-medium">{exchange.question}</p>
            <p className={`whitespace-pre-wrap leading-relaxed ${exchange.failed ? "text-red-700" : "text-zinc-700"}`}>
              {exchange.answer ?? "Menjawab…"}
            </p>
          </li>
        ))}
      </ol>
      <div className="flex flex-col gap-1 px-3 pt-2">
        <button
          onClick={() => void suggest()}
          disabled={!enabled || suggesting}
          className="self-start text-xs text-zinc-500 hover:text-zinc-900 disabled:opacity-50"
        >
          {suggesting ? "Mencari saran…" : "Beri saran pertanyaan"}
        </button>
        {suggestions.map((question) => (
          <button
            key={question}
            onClick={() => void submit(question)}
            disabled={waiting}
            className="rounded-md bg-zinc-100 px-2 py-1 text-left text-xs hover:bg-zinc-200 disabled:opacity-50"
          >
            {question}
          </button>
        ))}
      </div>
      <form onSubmit={submitDraft} className="flex gap-2 p-3">
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
