import { useCallback, useEffect, useState } from "react";
import {
  type ActionItem,
  extractActions,
  getWebhook,
  listActions,
  sendActions,
  setActionDone,
  setWebhook,
} from "./notes";

interface Props {
  onOpen: (meetingId: number) => void;
  /** The meeting that is open, if any: what "Baca ulang" and "Kirim" act on. */
  hasMeeting: boolean;
  idle: boolean;
}

/** Action items from all meetings, with an address to send them to. */
export function Actions({ onOpen, hasMeeting, idle }: Props) {
  const [items, setItems] = useState<ActionItem[]>([]);
  const [showDone, setShowDone] = useState(false);
  const [owner, setOwner] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [webhook, setWebhookText] = useState("");

  const refresh = useCallback(() => {
    listActions(showDone).then(setItems, (reason) => setMessage(String(reason)));
  }, [showDone]);
  useEffect(refresh, [refresh]);
  useEffect(() => {
    getWebhook().then(setWebhookText, () => {});
  }, []);

  async function run(work: () => Promise<void>, done: string) {
    setBusy(true);
    setMessage(null);
    try {
      await work();
      setMessage(done);
      refresh();
    } catch (reason) {
      setMessage(String(reason));
    }
    setBusy(false);
  }

  const owners = [...new Set(items.map((item) => item.owner).filter((name): name is string => !!name))].sort();
  const shown = owner ? items.filter((item) => item.owner === owner) : items;

  return (
    <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col gap-4 overflow-y-auto px-4 py-4">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <h2 className="mr-auto text-base font-medium">Tugas dari semua rapat</h2>
        <select
          value={owner}
          onChange={(event) => setOwner(event.target.value)}
          aria-label="Penanggung jawab"
          className="rounded-md border border-zinc-300 px-2 py-1"
        >
          <option value="">Semua orang</option>
          {owners.map((name) => (
            <option key={name}>{name}</option>
          ))}
        </select>
        <label className="flex items-center gap-1">
          <input type="checkbox" checked={showDone} onChange={(event) => setShowDone(event.target.checked)} />
          Tampilkan yang selesai
        </label>
      </div>

      {shown.length === 0 ? (
        <p className="text-sm text-zinc-500">
          Belum ada tugas. Tugas dicari otomatis setelah rapat selesai; rapat lama bisa dibaca ulang dari tombol di bawah.
        </p>
      ) : (
        <ul className="flex flex-col divide-y divide-zinc-100 rounded-lg border border-zinc-200">
          {shown.map((item) => (
            <li key={item.id} className="flex items-start gap-3 px-3 py-2 text-sm">
              <input
                type="checkbox"
                checked={item.done}
                aria-label={`Selesai: ${item.task}`}
                onChange={(event) => void setActionDone(item.id, event.target.checked).then(refresh)}
                className="mt-1"
              />
              <div className="min-w-0 flex-1">
                <p className={item.done ? "text-zinc-400 line-through" : ""}>{item.task}</p>
                <p className="text-xs text-zinc-500">
                  {item.owner ?? "Belum ada penanggung jawab"}
                  {item.due ? ` · tenggat ${item.due}` : ""} ·{" "}
                  <button onClick={() => onOpen(item.meeting_id)} className="underline hover:text-zinc-800">
                    {item.meeting_title || "Rapat"}
                  </button>
                </p>
              </div>
            </li>
          ))}
        </ul>
      )}

      <div className="flex flex-wrap gap-2 text-sm">
        <button
          onClick={() => void run(extractActions, "Tugas dibaca ulang dari rapat yang terbuka.")}
          disabled={busy || !hasMeeting || !idle}
          className="rounded-md border border-zinc-300 px-2 py-1 hover:bg-zinc-50 disabled:opacity-50"
        >
          Baca ulang tugas rapat yang terbuka
        </button>
        <button
          onClick={() => void run(sendActions, "Tugas rapat yang terbuka terkirim.")}
          disabled={busy || !hasMeeting || !webhook.trim()}
          className="rounded-md border border-zinc-300 px-2 py-1 hover:bg-zinc-50 disabled:opacity-50"
        >
          Kirim ke webhook
        </button>
      </div>
      {message && (
        <p role="status" className="text-xs text-zinc-600">
          {message}
        </p>
      )}

      <section className="flex flex-col gap-2 border-t border-zinc-200 pt-4 text-sm">
        <h3 className="font-medium">Webhook</h3>
        <p className="text-xs text-zinc-500">
          Setelah tiap rapat selesai, tugasnya dikirim ke alamat ini sebagai JSON (kolom <code>text</code> dan{" "}
          <code>items</code>). Cocok untuk Slack/Discord incoming webhook, Zapier, Make, n8n, atau automasi Notion dan Jira.
          Kosongkan untuk mematikan.
        </p>
        <div className="flex gap-2">
          <input
            value={webhook}
            onChange={(event) => setWebhookText(event.target.value)}
            placeholder="https://hooks.slack.com/services/…"
            className="min-w-0 flex-1 rounded-md border border-zinc-300 px-2 py-1"
          />
          <button
            onClick={() => void run(() => setWebhook(webhook), "Alamat webhook disimpan.")}
            disabled={busy}
            className="rounded-md bg-zinc-800 px-3 py-1 font-medium text-white hover:bg-zinc-900 disabled:opacity-50"
          >
            Simpan
          </button>
        </div>
      </section>
    </div>
  );
}
