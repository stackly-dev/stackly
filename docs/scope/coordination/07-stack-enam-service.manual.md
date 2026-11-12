# Langkah manual: `07-stack-enam-service`

Langkah yang tidak dijalankan sesi (GUI, unduhan runtime, sesi login launchd). Owner: user. Tiap langkah menyebut AC yang dibuktikannya; hasilnya dicatat di `verify.md` spec itu.

## Fitur 41 · Provider Traefik & routing lokal (spec 0041)

Prasyarat: milestone 3 spec 0041 sudah di `main` (Traefik `[[provider]]`), app dibangun dengan `cargo xtask build-app`, satu server HTTP lokal sebagai backend. Nilai dari user: URL backend lokal, port web kedua instance.

1. Di app: Catalog, pilih Traefik, pasang runtime 3.7.15 (unduhan), buat instance `edge-a` dan `edge-b`, Start keduanya. Harapan: keduanya ready. Bukti AC-10, AC-2.
2. `edge-a`, tab Configuration: tambah route Name `api`, Rule ``Host(`app.localhost`)``, Backend URL server lokal. Harapan: tag "Applies on reload", tombol Apply and Reload. Apply, lalu `curl -H 'Host: app.localhost' http://127.0.0.1:<port web edge-a>/` menjawab backend; perintah yang sama ke port web `edge-b` menjawab 404. Bukti AC-9, AC-2.
3. Ubah Log level. Harapan: tag "Requires restart", tombol Apply and Restart. Bukti AC-9.
4. Ganti Rule menjadi `Host(` lalu Apply. Harapan: kartu gagal, status not ready, banner "Revision N hasn't reached ready"; klik Roll Back to Revision M, route menjawab lagi. Bukti AC-4.
5. Menu Open In: Routes File membuka Finder di `routes.yaml`; Loaded Routers membuka JSON router di browser. Bukti AC-1.
6. Doctor saat langkah 4 sebelum roll back. Harapan: remedy "Traefik hasn't loaded the routes Stackly wrote". Bukti AC-13.
7. `cargo test -p stacklyd --test native_launchd -- --ignored traefik_survives` di sesi GUI yang login. Harapan: PASS, lalu baris `HANDOVER_PROVEN` boleh di-commit. Bukti AC-12. Tanpa sesi GUI atau izin: `BLOCKED`, owner user.
