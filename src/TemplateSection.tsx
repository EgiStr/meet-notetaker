import { useEffect, useState } from "react";
import { type CustomTemplate, getCustomTemplate, setCustomTemplate } from "./notes";

const field = "rounded-md border border-zinc-300 px-3 py-1.5 text-sm outline-none focus:border-zinc-500";

/** The user's own MoM template: its sections and extra instructions. Chosen as "Buatan saya" under "Jenis rapat". */
export function TemplateSection() {
  const [form, setForm] = useState<CustomTemplate>({ sections: "", instructions: "" });
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    getCustomTemplate().then(setForm, () => {});
  }, []);

  function save() {
    setCustomTemplate(form).then(
      () => setMessage("Tersimpan. Berlaku untuk MoM berikutnya dengan jenis rapat \"Buatan saya\"."),
      (reason) => setMessage(String(reason)),
    );
  }

  return (
    <section className="flex flex-col gap-3 rounded-lg border border-zinc-200 p-4">
      <div>
        <h3 className="text-sm font-medium">Template MoM buatan sendiri</h3>
        <p className="text-xs text-zinc-500">
          Tulis bagian MoM yang Anda mau, satu judul per baris, dan petunjuk untuk penulisnya. Kosongkan bagian untuk MoM
          bebas yang hanya mengikuti petunjuk.
        </p>
      </div>
      <textarea
        value={form.sections}
        onChange={(event) => setForm({ ...form, sections: event.target.value })}
        rows={5}
        placeholder={"Latar belakang\nKeputusan\nRisiko\nTindak lanjut"}
        aria-label="Bagian MoM"
        className={field}
      />
      <textarea
        value={form.instructions}
        onChange={(event) => setForm({ ...form, instructions: event.target.value })}
        rows={4}
        placeholder="Contoh: Tulis singkat, maksimal tiga butir per bagian. Sebut angka persis seperti di rapat."
        aria-label="Petunjuk tambahan"
        className={field}
      />
      <div className="flex items-center gap-3">
        <button onClick={save} className="rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-900">
          Simpan template
        </button>
        {message && (
          <p role="status" className="text-xs text-zinc-600">
            {message}
          </p>
        )}
      </div>
    </section>
  );
}
