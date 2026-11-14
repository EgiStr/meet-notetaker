# PRD: meet-notetaker

| | |
|---|---|
| Status | Draf v1, disetujui untuk tahap 0 |
| Tanggal | 2026-11-13 |
| Pemilik | Pengguna tunggal (alat pribadi) |

## 1. Masalah

Selama rapat, perhatian terbagi antara mengikuti pembicaraan dan mencatat. Setelah rapat, menyusun MoM memakan waktu dan sering tidak lengkap. Alat yang ada tidak cocok:

- Google AI Edge Foresight hanya tersedia untuk Mac Apple Silicon.
- Claude Meetings hanya untuk Google Meet, plan Team/Enterprise, dan memasukkan bot ke dalam call.
- tl;dv dan sejenisnya membutuhkan bot dan integrasi per platform.

## 2. Tujuan

1. Memahami rapat yang sedang berjalan lewat transkrip, MoM berkala, dan tanya-jawab.
2. Menghasilkan MoM terstruktur tanpa menulis manual.
3. Bekerja di semua platform rapat dan rapat tatap muka dengan satu mekanisme.
4. Bisa dipakai gratis, dengan pilihan penyedia berbayar bila diinginkan.

### Ukuran keberhasilan

| Ukuran | Target |
|---|---|
| Jeda transkrip live | 5 sampai 12 detik setelah diucapkan (dibatasi 20 permintaan per menit di Groq untuk dua stream) |
| MoM | Diperbarui kira-kira tiap 2 menit; versi akhir siap dalam 2 menit setelah rapat berhenti |
| Label pembicara akhir | Dapat dipakai tanpa koreksi besar pada rapat 2 sampai 8 pembicara (ambang angka ditetapkan setelah tahap 0) |
| Biaya default | Rp0 |

## 3. Bukan tujuan (MVP)

Mind map, pustaka pengetahuan lintas rapat, shorthand expansion, pembuatan dokumen/whiteboard, saran proaktif, pengenalan suara lintas rapat, pembatalan gema, capture audio per aplikasi, deteksi call otomatis, integrasi kalender, macOS/Linux, multi-pengguna, akun, sinkronisasi.

## 4. Pengguna dan lingkungan

- Satu pengguna, satu mesin Windows 11.
- Rapat berbahasa Indonesia campur Inggris, 2 sampai 8 pembicara.
- Rapat online lewat Teams, Zoom, atau Google Meet dengan headset; juga rapat tatap muka.

## 5. Alur utama

1. Pengguna membuka aplikasi, mengetik judul rapat, menekan Rekam (tombol atau hotkey global).
2. Transkrip mengalir di panel kiri; MoM diperbarui di panel tengah; pengguna bertanya di panel kanan dan menulis di "Catatan saya".
3. Pengguna menekan Berhenti. Aplikasi menjalankan diarization akhir, memperbaiki label pembicara, dan menyusun ulang MoM dari seluruh transkrip.
4. Pengguna mengganti nama "Pembicara 2" menjadi nama asli, mengedit MoM, lalu mengekspor ke Markdown.
5. Rapat tersimpan di daftar riwayat dan bisa dicari.

Alur kedua: mengimpor file rekaman dan memprosesnya dengan pipeline yang sama tanpa batasan realtime.

## 6. Kebutuhan fungsional

### Tangkapan audio

| ID | Kebutuhan |
|---|---|
| F-01 | Merekam mikrofon dan seluruh audio keluaran sistem sebagai dua stream terpisah. |
| F-02 | Mulai dan berhenti manual lewat tombol dan hotkey global. |
| F-03 | Perangkat input dan output dapat dipilih di Settings. |
| F-04 | Menampilkan peringatan jika perangkat output aktif bukan headset. |
| F-05 | Mode "rapat tatap muka": hanya mikrofon, diarization dijalankan pada mikrofon. |
| F-06 | Menyimpan audio per rapat dan mengompresnya setelah rapat selesai. |

### Transkripsi dan pembicara

| ID | Kebutuhan |
|---|---|
| F-10 | Penyedia transkripsi dipilih di Settings: endpoint OpenAI-compatible (default: Groq `whisper-large-v3-turbo`), Deepgram, Whisper lokal (pilihan offline). |
| F-17 | Pengiriman ke STT cloud menghormati batas penyedia: potongan audio digabung sehingga jumlah permintaan per menit tidak melampaui batas, dan potongan yang gagal diantrekan ulang tanpa menghentikan rekaman. |
| F-11 | Penyedia diarization dipilih di Settings; dilewati bila penyedia transkripsi sudah mengembalikan label pembicara. |
| F-12 | Segmen dari mikrofon selalu dilabeli nama pengguna. |
| F-13 | Segmen dari audio sistem dilabeli Pembicara 1…n; label live bersifat sementara. |
| F-14 | Setelah rapat berhenti, diarization akhir dijalankan dan label di transkrip diperbarui. |
| F-15 | Bila mesin tidak sanggup realtime, label live turun menjadi "Saya"/"Lainnya" tanpa memperlambat transkrip. |
| F-16 | Pengguna dapat mengganti nama pembicara; berlaku di seluruh transkrip dan MoM rapat itu. |

### MoM dan tanya-jawab

| ID | Kebutuhan |
|---|---|
| F-20 | Satu adapter LLM berformat OpenAI-compatible (base URL, API key, model); default Gemini Flash free tier. |
| F-21 | MoM diperbarui kira-kira tiap 2 menit secara inkremental dengan bagian: Ringkasan, Topik, Keputusan, Action Items, Pertanyaan Terbuka. |
| F-22 | Saat rapat berhenti, MoM disusun ulang penuh dari seluruh transkrip. |
| F-23 | MoM hanya-baca selama rapat; dapat diedit setelah rapat. |
| F-24 | Kolom "Catatan saya" ikut dikirim ke LLM sebagai petunjuk. |
| F-25 | Tanya-jawab hanya saat diminta, dengan seluruh transkrip dan MoM terkini sebagai konteks. |
| F-26 | Output MoM dan jawaban dalam bahasa Indonesia. |
| F-27 | Saat kuota habis atau offline: rekaman dan transkrip tetap berjalan, pembaruan MoM diantrekan, status "MoM tertunda" tampil. Tidak ada perpindahan otomatis ke penyedia berbayar. |

### Penyimpanan dan riwayat

| ID | Kebutuhan |
|---|---|
| F-30 | Rapat, segmen transkrip, MoM, dan chat disimpan di SQLite lokal. |
| F-31 | Daftar rapat (judul, tanggal, durasi) dengan pencarian teks di judul dan transkrip. |
| F-32 | Ekspor MoM ke Markdown. |
| F-33 | Impor file audio/video dan proses dengan pipeline yang sama. |
| F-34 | API key disimpan di Windows Credential Manager, tidak di file konfigurasi atau basis data. |

## 7. Kebutuhan non-fungsional

- **Privasi:** dengan pengaturan default, potongan audio rapat dikirim ke penyedia STT cloud dan teks transkrip dikirim ke penyedia LLM. Keduanya ditampilkan jelas di Settings. Audio hanya tetap di mesin jika pengguna memilih Whisper lokal.
- **Ketahanan:** kegagalan jaringan atau LLM tidak boleh menghentikan rekaman atau menghilangkan transkrip.
- **Kinerja:** transkripsi live tidak boleh tertinggal makin jauh seiring waktu; jika antrean audio menumpuk, fitur opsional (label live) dimatikan lebih dulu.
- **Persetujuan:** tidak ada bot yang terlihat, sehingga memberi tahu peserta adalah tanggung jawab pengguna. Aplikasi menampilkan pengingat saat rekaman dimulai.

## 8. Arsitektur dan prinsip rekayasa

Tauri 2. Inti Rust menangani audio, transkripsi, diarization, LLM, dan penyimpanan. UI memakai React, TypeScript, Vite, Tailwind, shadcn/ui.

```
capture (mic, sistem) -> VAD -> Transcriber -> Diarizer (opsional) -> store -> UI
                                                       |
                                                 MoM / tanya-jawab (LlmClient)
```

Prinsip yang mengikat keputusan kode:

- **Satu tanggung jawab per modul:** capture, transkripsi, diarization, LLM, dan penyimpanan terpisah; tidak ada modul yang tahu detail modul lain.
- **Bergantung pada abstraksi:** pipeline hanya mengenal trait `Transcriber`, `Diarizer`, `LlmClient`. Penyedia konkret dipilih di satu tempat (composition root) dari Settings.
- **Terbuka untuk penyedia baru tanpa mengubah pipeline:** penyedia baru berarti satu implementasi trait baru.
- **Trait kecil:** tiap trait hanya memuat method yang benar-benar dipakai pipeline.
- **YAGNI:** trait baru dibuat ketika ada implementasi kedua yang nyata. Tidak ada sistem plugin dinamis, tidak ada konfigurasi untuk hal yang belum diminta.
- **Kode bersih:** nama menjelaskan maksud, fungsi pendek, kesalahan dikembalikan sebagai `Result` bertipe dan tidak ditelan, tanpa `unwrap` di jalur produksi.
- **Uji lebih dulu untuk logika murni:** pemotongan segmen, penggabungan label pembicara, penyusunan prompt inkremental, dan antrean ulang ditulis dengan uji sebelum implementasi. Audio dan jaringan diuji lewat implementasi palsu dari trait.

## 9. Tahapan dan kriteria selesai

| Tahap | Isi | Selesai jika |
|---|---|---|
| 0 | Uji kecil Whisper dan diarization pada rekaman contoh | Ada angka kecepatan (rasio terhadap realtime) dan penilaian akurasi untuk tiap kandidat, dan pilihan pustaka lokal dikonfirmasi atau diganti |
| 1 | Capture audio dan transkrip live | Rapat sungguhan 30 menit ditranskrip dengan jeda sesuai target tanpa menumpuk |
| 2 | MoM berkala dan tanya-jawab | MoM terisi selama rapat dan pertanyaan atas isi rapat terjawab dengan benar |
| 3 | Diarization akhir, ganti nama pembicara, halaman rapat | Label akhir muncul setelah rapat dan penggantian nama berlaku di transkrip dan MoM |
| 4 | Impor file, adapter Deepgram dan OpenAI-compatible | File rekaman diproses penuh; ketiga penyedia transkripsi dapat dipilih dan berfungsi |

## 10. Risiko

| Risiko | Dampak | Penanganan |
|---|---|---|
| Whisper lokal lambat di CPU ini (tahap 0: `small` 0,34x, `base` 1,26x realtime; penyebab belum ditemukan) | Tidak layak untuk live | Diputuskan: STT cloud menjadi default; Whisper lokal hanya pilihan offline |
| Batas Groq (20 permintaan/menit, 7.200 detik audio/jam, 28.800 detik/hari menurut halaman rate limit; belum jelas apakah sama untuk akun gratis) | Dua stream penuh tepat di batas per jam; rapat lebih dari 4 jam sehari melampaui batas harian | Buang jeda hening dengan VAD sebelum mengirim, gabungkan potongan (F-17), tampilkan sisa kuota |
| Audio rapat keluar dari mesin | Rapat rahasia terekspos ke pihak ketiga | Ditampilkan di Settings; Whisper lokal tersedia untuk rapat sensitif |
| Akurasi diarization lokal rendah untuk bahasa Indonesia | Label pembicara tidak dapat dipakai | Diukur di tahap 0; cadangan: crate lain atau diarization dari Deepgram |
| Angka kinerja crate hanya dari README | Keputusan pustaka keliru | Tidak ada pustaka yang dikunci sebelum tahap 0 selesai |
| Kuota free tier LLM habis di tengah rapat | MoM tertunda | Pembaruan inkremental, antrean ulang (F-27) |
| Tanpa headset, suara peserta masuk ke mikrofon | Ucapan ganda di transkrip | Peringatan (F-04); pembatalan gema di fase 2 |
| Merekam tanpa sepengetahuan peserta | Masalah etika dan hukum | Pengingat saat mulai merekam |

## 10a. Hasil tahap 0 (2026-11-13)

Diukur di Ryzen 5 7535HS, 16 GB, tanpa GPU diskrit.

| Kandidat | Audio uji | Kecepatan | Catatan |
|---|---|---|---|
| Groq `whisper-large-v3-turbo` | Narasi Indonesia 9,7 menit (mp3 9 MB) | 5,2x realtime (111 detik, sebagian besar waktu unggah) | Teks rapi, tanda baca benar |
| Groq `whisper-large-v3-turbo` | Sintetis Inggris 70 detik | 14,3x realtime (4,9 detik) | Benar kata per kata |
| Whisper lokal `small` q5_1 | Sintetis Inggris 70 detik | 0,34x realtime | Tidak layak untuk live |
| Whisper lokal `base` q5_1 | Sintetis Inggris 70 detik | 1,26x realtime | Margin terlalu tipis |
| polyvoice 1.0.0 | Narasi Indonesia 9,7 menit | 17,4x realtime | 1 pembicara terdeteksi |
| speakrs 0.7.0 | Narasi Indonesia 9,7 menit | 1,6x realtime | 1 pembicara terdeteksi |
| STT bawaan Edge 157 | Sintetis Inggris, 3 cara | - | Semua hasil teks kosong, berakhir error `network` |

Belum terukur: akurasi diarization pada rekaman nyata dengan 3 pembicara atau lebih. Pada audio sintetis 5 suara TTS, kedua diarizer sama-sama menemukan 3 pembicara.

## 11. Keputusan yang masih terbuka

- Ambang angka untuk "akurasi diarization dapat dipakai" (setelah tahap 0).
- Format kompresi audio: Opus atau FLAC (diputuskan di tahap 1).

## 12. Rujukan

Dokumentasi resmi Edge Foresight tidak dapat dibaca saat riset (iframe mengembalikan 502); rincian fiturnya berasal dari liputan pers.

- https://www.eweek.com/news/google-ai-edge-foresight-mac/
- https://talat.app/blog/google-ai-edge-foresight
- https://techcrunch.com/2026/11/10/anthropic-launches-claude-meetings-to-turn-workplace-discussions-into-plans-and-tasks/
- https://the-decoder.com/claude-meetings-lets-anthropics-ai-join-your-video-calls/
