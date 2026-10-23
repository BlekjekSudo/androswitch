# AndroSwitch: mulai dari satu NRO

Ini paket pengembangan M7. Pengujian regresi dilakukan di Ryujinx. Pengguna
melaporkan empat contoh bisa dibuka di Switch asli, termasuk animasi dan beep;
tetapi Plus pada contoh native crash. Perubahan penutupan dalam build ini
masih perlu dites ulang di Switch. Fondasinya adalah **RustyDroid karya nadabgfccvd dan para
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
APK tunggal maksimal 64 MiB, minSdk maksimal 26, native ARM64, offline. Belum
ada dukungan Google Play Services, split APK atau jaminan engine/game umum.
Pakai empat contoh APK untuk pengujian awal di perangkat.

Jika worker native tidak mau berhenti, runtime mencatat alasan lalu menutup
proses Horizon. Buka ulang homebrew untuk melihat pemberitahuan gangguan.
Simpan folder `logs/` untuk laporan. `session-report.json` kini dibuat saat
peluncuran biasa juga. `teardown.log` mencatat tahap penutupan sebelum operasi
dijalankan; baris terakhir membantu melacak crash Plus. Jika berhasil kembali,
baris terakhir adalah `launcher_ready`. Detail cakupan dan pengujian ada di
[panduan pemakaian](USING_ANDROSWITCH.md) dan [bukti M6/M7](M6_M7_E2E.md).
