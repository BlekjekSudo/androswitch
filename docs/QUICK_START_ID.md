# AndroSwitch: mulai dari satu NRO

Ini kandidat validasi M9. Baseline M1–M7 sudah dikonfirmasi diuji di Switch asli.
Build baru dengan storage/M8/M9 belum diuji ulang di Ryujinx atau Switch.
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
Kandidat ini menyertakan tujuh contoh: empat contoh sebelumnya, preferences,
catatan SQLite, dan HelloActivity asli AOSP. Buka preferences/catatan beberapa
kali untuk memeriksa data tetap tersimpan, lalu periksa kembali tombol Plus.

Batas 64 MiB pada alpha lama sudah diganti pembacaan APK bertahap. Batas container
kini 512 MiB; entry yang dimuat utuh tetap maksimal 64 MiB, total DEX mentah
32 MiB, database SQLite 8 MiB. Heap runtime 256 MiB juga dipakai VM, library
dan media, sehingga APK lebih besar belum tentu bisa berjalan. OBB dibaca bertahap.
Mekorama 1.7.2 berukuran sekitar 12,5 MiB, tetapi gagal karena dependency native
`libstdc++.so` belum tersedia. Runtime sekarang membersihkan startup gagal sebelum
kembali ke console bila aman; ini belum berarti Mekorama bisa dimainkan.
Lihat [diagnosis dan pengujian startup](https://github.com/BlekjekSudo/androswitch/blob/main/docs/STARTUP_FAILURES.md).

Jika worker native tidak mau berhenti, runtime mencatat alasan lalu menutup
proses Horizon. Buka ulang homebrew untuk melihat pemberitahuan gangguan.
Simpan folder `logs/` untuk laporan. `session-report.json` kini dibuat saat
peluncuran biasa juga. `teardown.log` mencatat tahap penutupan sebelum operasi
dijalankan; baris terakhir membantu melacak crash Plus. Jika berhasil kembali,
jalur grafik mencatat `launcher_ready` untuk frame pertama dan `launcher_stable`
setelah delapan frame. Jika masih gagal, simpan juga `display-handoffs.log`
dan `display-error.json` bila ada. Detail cakupan dan pengujian ada di
[panduan pemakaian](USING_ANDROSWITCH.md) dan [bukti M6/M7](M6_M7_E2E.md).
