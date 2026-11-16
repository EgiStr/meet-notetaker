# Meet Notetaker

Aplikasi desktop Windows untuk mencatat rapat: transkrip langsung, notulen (MoM) yang diperbarui selama rapat,
tanya jawab dengan AI, label pembicara, dan pencarian lintas rapat. Gabungan ide tl;dv dan Google Foresight
dalam satu aplikasi open source. Rekaman, transkrip, dan notulen disimpan di komputer Anda.

Dibangun dengan Rust (Tauri 2) dan React. Desain lengkap ada di [docs/PRD.md](docs/PRD.md).

## Fitur

- Rekam mikrofon dan audio sistem, transkrip langsung.
- Notulen berkala dan akhir, dengan template: umum, standup, one-on-one, penjualan, retro.
- Catatan pribadi yang bisa diperluas AI, pertanyaan yang disarankan, tanya jawab saat rapat.
- Label pembicara otomatis (diarization) dan nama pembicara yang bisa diubah.
- Riwayat rapat (SQLite), pencarian, tanya ke semua rapat, penanda, ekspor Markdown, hapus.
- Impor rekaman audio yang sudah ada.
- Transkripsi: layanan yang kompatibel dengan OpenAI (mis. Groq), Deepgram, atau Whisper lokal.
- Model AI bebas dipilih: tambah penyedia dan model di layar **Model**.

## Model AI

Buka **Model** di aplikasi, tambah penyedia (alamat dasar API yang kompatibel dengan OpenAI), lalu tambah atau
ambil daftar model dan pilih yang dipakai. Contoh: Gemini, OpenAI, Groq, OpenRouter, gateway
[9router](https://github.com/decolua/9Router) (`http://localhost:20128/v1`), atau Ollama lokal
(`http://localhost:11434/v1`). Model bisa diganti kapan saja, juga di tengah rapat.

Kunci API disimpan di Windows Credential Manager, bukan di berkas. Daftar penyedia dan model ada di
`providers.json` di folder data aplikasi.

Selama belum ada model yang dipilih, aplikasi memakai variabel lingkungan `LLM_API_KEY`, `LLM_BASE_URL`
(bawaan Gemini) dan `LLM_MODEL`.

## Menjalankan

Butuh Rust, Node.js, dan prasyarat Tauri 2 untuk Windows.

```bash
npm install
npx tauri dev
```

Transkripsi diatur lewat variabel lingkungan: `STT_PROVIDER` (`openai`, `deepgram`, `whisper`), `STT_API_KEY`,
`STT_BASE_URL`, `STT_MODEL`, `STT_LANGUAGE`, `DEEPGRAM_API_KEY`, `WHISPER_MODEL_PATH`. Whisper lokal memakai
fitur `local-whisper` dan butuh cmake serta LLVM saat build.

Tes: `cargo test -p notetaker-core`.

## Privasi dan izin

Aplikasi merekam suara. Beri tahu peserta rapat dan patuhi hukum setempat sebelum merekam. Audio dan model
tidak pernah masuk git (lihat `.gitignore`).

## Lisensi

[Apache-2.0](LICENSE). Audit lisensi dependensi Rust: tidak ada GPL atau AGPL. Sebagian kecil memakai MPL-2.0
(mis. `symphonia`), yang boleh dipakai komersial selama berkas MPL yang diubah tetap terbuka; kode ini tidak
mengubahnya. File model (Whisper, diarization) punya lisensi sendiri; periksa sebelum mendistribusikannya.
