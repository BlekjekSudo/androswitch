# AndroSwitch: mulai dari satu NRO

Ini kandidat validasi M9. Runtime terbaru memakai Rust `std` di Horizon melalui
PAL libnx/newlib yang dipin. Build `std` ini sudah diuji di Ryujinx dan masih
memerlukan tes Switch fisik; lihat [laporan migrasi](HORIZON_STD_MIGRATION.md).
Baseline M1–M7 sudah dikonfirmasi diuji di Switch asli.
Kandidat sebelumnya juga mendapat laporan bahwa instalasi APK besar, SQLite,
dan SharedPreferences berjalan. Perubahan memori/native/GLSurfaceView yang baru
belum diuji di Switch; kedua game belum memiliki bukti layar pertama.
Pengguna sebelumnya
melaporkan empat contoh bisa dibuka di Switch asli, termasuk animasi dan beep;
pengguna kemudian mengonfirmasi perbaikan display menyelesaikan crash Plus dan
tersendat saat awal membuka contoh native. Perubahan pemulihan startup terbaru
masih perlu dites di Switch. Fondasinya adalah **RustyDroid karya nadabgfccvd dan para
kontributornya**. Kredit dan lisensi lengkap ikut disertakan dalam paket.

1. Ekstrak folder `switch/` dari ZIP ke SD. Jangan hapus folder data lama saat
   memperbarui runtime.
2. Buka `switch/androswitch/androswitch.nro` melalui application/title override.
3. Tekan R ke daftar APKs, pilih contoh, lalu A untuk install. Aplikasi langsung
   dipilih di Installed; tekan A lagi untuk menjalankannya.
4. Tekan Plus di aplikasi untuk kembali ke console. Plus di console keluar
   dari homebrew.

Taruh APK lain di `switch/androswitch/apks/`, kemudian tekan Minus untuk scan
ulang. Install APK yang identik mempertahankan revision; APK yang berubah
mendapat revision baru dengan save tetap disimpan. X membuka detail, Y
menghapus aplikasi dengan save tetap disimpan. ZR menghapus data internal
melalui konfirmasi terpisah. Sentuh baris untuk memilih; sentuh lagi untuk buka.

OBB masuk ke `Android/obb/<nama.paket>/`, data eksternal ke
`Android/data/<nama.paket>/`. Foto dan unduhan memakai `DCIM/` dan `Download/`.
Save internal ada di `userdata/<nama.paket>/`.

APK umum masih bisa gagal karena API Android yang belum tersedia. Batas awal:
APK tunggal maksimal **512 MiB**, minSdk maksimal 26, native ARM64, offline. Belum
ada dukungan Google Play Services, split APK atau jaminan engine/game umum.
Paket migrasi `std` menyertakan satu **AndroSwitchConformance.apk**, dengan
219 kasus tanpa sensor dalam satu aplikasi. Install, buka, lalu pilih Run all;
tes bisa dilanjutkan melalui jurnal setelah aplikasi dibuka ulang. Pilih Run
category untuk mengulang kelompok tertentu. Hasil ada di
`userdata/org.androswitch.conformance/files/`. Ini suite proyek, bukan CTS resmi.
Ryujinx mencatat 216 PASS dan tiga query clock PCV NOT_RUN; hasil Switch fisik
harus dicatat tersendiri. Contoh kecil lama tetap tersedia sebagai fixture
pengembang, dan hanya ikut paket jika dipilih ketika mengemas.

Batas container kini 512 MiB dengan pembacaan bertahap. Metadata/resource yang
dimuat utuh tetap dibatasi 64 MiB; native memakai anggaran terpisah hingga
128 MiB. DEX diperiksa berdasarkan estimasi raw dan decoded metadata, dengan
batas parser hingga 256 MiB serta cadangan allocator. Heap mengikuti loader
atau kapasitas proses yang diukur; 256 MiB adalah default lama, bukan kapasitas
yang selalu dipakai. Database SQLite tetap 8 MiB; OBB dibaca bertahap.
ABI C++ bersama dan GLSurfaceView baru menambah jalur startup untuk evaluasi
Mekorama/Candy Crush; ini belum berarti kedua game sudah terbukti terbuka.
Lihat [diagnosis dan pengujian startup](https://github.com/BlekjekSudo/androswitch/blob/main/docs/STARTUP_FAILURES.md).

Untuk tes kandidat ini, pertahankan data lama, lalu coba Mekorama dan Candy
Crush dari APK milikmu. Keduanya tidak dibundel. Setelah setiap percobaan,
salin folder `logs/` sebelum mencoba game berikutnya agar laporan sesi tidak
tertimpa. Catat layar terakhir yang terlihat dan hasil tombol Plus.
`memory.log`, `memory-admission.json`, `native-compatibility.json` dan
`session-report.json` membantu memisahkan batas memori dari API yang belum ada.

Jika worker native tidak mau berhenti, runtime mencatat alasan lalu menutup
proses Horizon. Buka ulang homebrew untuk melihat pemberitahuan gangguan.
Simpan folder `logs/` untuk laporan. `session-report.json` kini dibuat saat
peluncuran biasa juga. `teardown.log` mencatat tahap penutupan sebelum operasi
dijalankan; baris terakhir membantu melacak crash Plus. Jika berhasil kembali,
jalur grafik mencatat `launcher_ready` untuk frame pertama dan `launcher_stable`
setelah delapan frame. Jika masih gagal, simpan juga `display-handoffs.log`
dan `display-error.json` bila ada. Detail cakupan dan pengujian ada di
[panduan pemakaian](USING_ANDROSWITCH.md) dan [bukti M6/M7](M6_M7_E2E.md).
