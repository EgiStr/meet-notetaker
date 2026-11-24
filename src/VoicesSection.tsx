import { useCallback, useEffect, useState } from "react";
import { forgetVoice, listVoices } from "./notes";

/** The voices the app remembers, so a name can be forgotten. */
export function VoicesSection() {
  const [names, setNames] = useState<string[]>([]);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(() => {
    listVoices().then(setNames, (reason) => setMessage(String(reason)));
  }, []);
  useEffect(refresh, [refresh]);

  async function forget(name: string) {
    if (!window.confirm(`Lupakan suara "${name}"? Rapat berikutnya tidak lagi memberi nama ini otomatis.`)) return;
    try {
      await forgetVoice(name);
      refresh();
    } catch (reason) {
      setMessage(String(reason));
    }
  }

  return (
    <section className="flex flex-col gap-3 rounded-lg border border-zinc-200 p-4">
      <div>
        <h3 className="text-sm font-medium">Suara yang diingat</h3>
        <p className="text-xs text-zinc-500">
          Sidik suara orang yang pernah Anda beri nama, disimpan hanya di komputer ini. Hapus bila salah atau tidak
          diperlukan lagi; nama di rapat lama tidak berubah.
        </p>
      </div>
      {names.length === 0 ? (
        <p className="text-xs text-zinc-500">Belum ada suara yang diingat.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-zinc-100 text-sm">
          {names.map((name) => (
            <li key={name} className="flex items-center justify-between py-1.5">
              <span>{name}</span>
              <button
                onClick={() => void forget(name)}
                aria-label={`Lupakan suara ${name}`}
                className="rounded-md border border-zinc-300 px-2 py-0.5 text-xs hover:bg-zinc-50"
              >
                Lupakan
              </button>
            </li>
          ))}
        </ul>
      )}
      {message && (
        <p role="status" className="text-xs text-zinc-600">
          {message}
        </p>
      )}
    </section>
  );
}
