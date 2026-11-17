import type { ReactNode } from "react";
import { rebuildMom } from "./notes";

interface Props {
  mom: string;
  pending: string | null;
  /** Retrying by hand only makes sense once the periodic updates have stopped. */
  canRetry: boolean;
  /** What is being discussed right now, while recording. */
  brief?: string;
}

export function MomPanel({ mom, pending, canRetry, brief }: Props) {
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      {pending && (
        <p role="status" className="border-b border-amber-200 bg-amber-50 px-4 py-2 text-xs text-amber-900">
          Ringkasan tertunda: {pending}{" "}
          {canRetry && (
            <button onClick={() => void rebuildMom()} className="font-medium underline">
              Coba lagi
            </button>
          )}
        </p>
      )}
      <div className="flex-1 overflow-y-auto px-4 py-4 text-sm">
        {brief && (
          <div className="mb-4 rounded-lg bg-indigo-50 px-3 py-2 text-indigo-950">
            <p className="mb-1 text-xs font-medium text-indigo-700">Sedang dibahas</p>
            <p className="whitespace-pre-wrap leading-relaxed">{brief}</p>
          </div>
        )}
        {mom ? (
          <Markdown text={mom} />
        ) : (
          <p className="text-zinc-500">Ringkasan muncul kira-kira dua menit setelah rekaman dimulai.</p>
        )}
      </div>
    </section>
  );
}

/** Renders the only Markdown the minutes use: `##` headings and `-` bullets. */
function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  let bullets: string[] = [];
  const closeList = () => {
    if (bullets.length === 0) return;
    blocks.push(
      <ul key={blocks.length} className="mb-3 list-disc space-y-1 pl-5 leading-relaxed">
        {bullets.map((bullet, index) => (
          <li key={index}>{bullet}</li>
        ))}
      </ul>,
    );
    bullets = [];
  };

  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (/^[-*] /.test(trimmed)) {
      bullets.push(trimmed.slice(2));
      continue;
    }
    closeList();
    if (trimmed.startsWith("#")) {
      blocks.push(
        <h3 key={blocks.length} className="mb-1 font-semibold text-zinc-900">
          {trimmed.replace(/^#+\s*/, "")}
        </h3>,
      );
    } else if (trimmed) {
      blocks.push(
        <p key={blocks.length} className="mb-2 leading-relaxed">
          {trimmed}
        </p>,
      );
    }
  }
  closeList();
  return <>{blocks}</>;
}
