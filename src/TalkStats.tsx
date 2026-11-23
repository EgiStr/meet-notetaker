import { useEffect, useState } from "react";
import { type TalkStat, talkStats } from "./notes";
import { formatClock } from "./recording";

/** Who spoke how much. Remount (change `key`) to show another meeting. */
export function TalkStats({ enabled }: { enabled: boolean }) {
  const [stats, setStats] = useState<TalkStat[]>([]);

  function refresh() {
    talkStats().then(setStats, () => setStats([]));
  }
  useEffect(() => {
    if (enabled) refresh();
  }, [enabled]);

  if (stats.length === 0) return null;
  return (
    <div className="border-t border-zinc-200">
      <div className="flex items-baseline justify-between px-4 pt-3">
        <h3 className="text-xs font-medium text-zinc-500">Porsi bicara</h3>
        <button onClick={refresh} className="text-xs text-zinc-500 underline hover:text-zinc-800">
          Segarkan
        </button>
      </div>
      <ul className="flex flex-col gap-2 px-4 py-2 text-sm">
        {stats.map((stat) => (
          <li key={stat.name}>
            <div className="flex items-baseline justify-between gap-2">
              <span className="min-w-0 truncate font-medium">{stat.name}</span>
              <span className="font-mono text-xs tabular-nums text-zinc-500">
                {Math.round(stat.share * 100)}% · {formatClock(stat.seconds)}
              </span>
            </div>
            <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-zinc-100" aria-hidden>
              <div className="h-full rounded-full bg-indigo-500" style={{ width: `${Math.round(stat.share * 100)}%` }} />
            </div>
            <p className="mt-1 text-xs text-zinc-500">
              {stat.words} kata · {Math.round(stat.words_per_minute)} kata/menit · {stat.questions} pertanyaan · terpanjang{" "}
              {formatClock(stat.longest_secs)}
            </p>
          </li>
        ))}
      </ul>
    </div>
  );
}
