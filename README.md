# Hozin Vector

Ubah foto jadi vektor SVG — siap dianimasikan di Alight Motion.
Native Kotlin, offline penuh, tanpa iklan, tanpa login.

## Cara pakai

1. Pilih foto dari galeri (mis. logo).
2. Atur **jumlah warna** (2–16) dan **detail**.
3. Tekan **Proses** — foto di-trace jadi SVG, satu path per warna.
4. **Salin** teks SVG, **Simpan** ke Downloads, atau **Bagikan**
   ke Alight Motion / aplikasi lain.

Di Alight Motion, tiap path jadi satu layer — tiap bagian logo
bisa digerakkan sendiri (mis. animasi logo digambar garis demi garis).

## Mesin

- Rust (`app/src/main/rust/vectrace`, crate `vectrace`):
  kuantisasi warna median-cut ke N warna persis, lalu
  [vtracer 0.6](https://github.com/visioncortex/vtracer)
  (spline) jadi SVG. Satu `<path>` per layer warna.
- JNI: `VectorEngine.trace()` mengembalikan JSON
  `{"svg","paths","colors","anchors"}`; null bila gagal.
- Preview SVG di-render ke bitmap pakai
  [AndroidSVG](https://github.com/BigBadaboom/androidsvg) — tanpa WebView.

## Privasi

Offline penuh: aplikasi tidak punya izin internet dan foto tidak
pernah dikirim ke mana pun. Semua proses jalan di perangkat.

## Build

APK rilis HANYA dibangun di GitHub Actions (workflow
`.github/workflows/android.yml`): tiap push ke `main` yang lolos
build otomatis membuat tag `v<versionName>` + GitHub Release berisi
APK-nya. `debug.keystore` persisten dipulihkan dari secret repo
`DEBUG_KEYSTORE_BASE64` supaya tanda tangan stabil antar versi —
file-nya di-`.gitignore`, jangan pernah push.

Build lokal hanya untuk cek porting, bukan untuk disalurkan:

```sh
./gradlew assembleDebug
```
