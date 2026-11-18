// Hozin Vector — mesin trace foto -> SVG (Rust, JNI).
//
// Alur: piksel RGBA dari Kotlin -> kuantisasi median-cut ke N warna
// (slider "jumlah warna") -> vtracer (spline) -> satu <path> SVG per
// layer warna, supaya tiap path jadi layer sendiri saat diimpor ke
// Alight Motion. Hasil dikembalikan sebagai JSON:
//   {"svg":"...","paths":N,"colors":N,"anchors":N}
// anchors = jumlah segmen C/L di atribut d (perkiraan titik anchor).
//
// JNI: semua export membungkus panic (catch_unwind) dan mengembalikan
// null bila gagal — Kotlin menampilkan pesan galat, bukan crash.

use jni::objects::{JByteArray, JObject};
use jni::sys::{jint, jstring};
use jni::JNIEnv;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use visioncortex::ColorImage;
use vtracer::{ColorMode, Config, Hierarchical};

/// Kuantisasi median-cut: tepat N warna (atau lebih sedikit bila gambar
/// memang punya warna lebih sedikit). Palet = rata-rata tertimbang tiap kotak.
fn median_cut(pixels: &[[u8; 3]], n: usize) -> Vec<[u8; 3]> {
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for p in pixels {
        *counts.entry(*p).or_insert(0) += 1;
    }
    if counts.len() <= n {
        return counts.keys().cloned().collect();
    }
    let mut boxes: Vec<Vec<([u8; 3], u32)>> = vec![counts.into_iter().collect()];
    while boxes.len() < n {
        // Kotak dengan rentang warna terbesar.
        let mut best = 0usize;
        let mut best_range = 0u16;
        for (i, b) in boxes.iter().enumerate() {
            for ch in 0..3 {
                let (mut lo, mut hi) = (255u16, 0u16);
                for (c, _) in b {
                    lo = lo.min(c[ch] as u16);
                    hi = hi.max(c[ch] as u16);
                }
                if hi - lo > best_range {
                    best_range = hi - lo;
                    best = i;
                }
            }
        }
        if best_range == 0 {
            break; // semua kotak sudah satu warna
        }
        let b = boxes.remove(best);
        let mut ch_best = 0;
        let mut range_best = 0u16;
        for ch in 0..3 {
            let (mut lo, mut hi) = (255u16, 0u16);
            for (c, _) in &b {
                lo = lo.min(c[ch] as u16);
                hi = hi.max(c[ch] as u16);
            }
            if hi - lo > range_best {
                range_best = hi - lo;
                ch_best = ch;
            }
        }
        let mut sorted = b;
        sorted.sort_by_key(|(c, _)| c[ch_best]);
        let total: u32 = sorted.iter().map(|(_, n)| *n).sum();
        let mut acc = 0u32;
        let mut cut = sorted.len() / 2;
        for (i, (_, cnt)) in sorted.iter().enumerate() {
            acc += cnt;
            if acc >= total / 2 {
                cut = (i + 1).min(sorted.len() - 1);
                break;
            }
        }
        let right = sorted.split_off(cut);
        boxes.push(sorted);
        boxes.push(right);
    }
    boxes
        .iter()
        .map(|b| {
            let (mut sr, mut sg, mut sb, mut tot) = (0u64, 0u64, 0u64, 0u64);
            for (c, n) in b {
                sr += c[0] as u64 * *n as u64;
                sg += c[1] as u64 * *n as u64;
                sb += c[2] as u64 * *n as u64;
                tot += *n as u64;
            }
            [(sr / tot) as u8, (sg / tot) as u8, (sb / tot) as u8]
        })
        .collect()
}

fn nearest(p: [u8; 3], palette: &[[u8; 3]]) -> [u8; 3] {
    let mut best = palette[0];
    let mut best_d = u32::MAX;
    for q in palette {
        let dr = p[0] as i32 - q[0] as i32;
        let dg = p[1] as i32 - q[1] as i32;
        let db = p[2] as i32 - q[2] as i32;
        let d = (dr * dr + dg * dg + db * db) as u32;
        if d < best_d {
            best_d = d;
            best = *q;
        }
    }
    best
}

/// Hitung segmen C/L di dalam atribut d="..." saja (bukan di fill="#...").
fn count_anchors(svg: &str) -> usize {
    let mut n = 0;
    let mut rest = svg;
    while let Some(i) = rest.find("d=\"") {
        rest = &rest[i + 3..];
        if let Some(j) = rest.find('"') {
            n += rest[..j].bytes().filter(|b| *b == b'C' || *b == b'L').count();
            rest = &rest[j + 1..];
        } else {
            break;
        }
    }
    n
}

fn trace_inner(
    rgba: &[u8],
    width: usize,
    height: usize,
    colors: usize,
    detail: f64,
) -> Result<String, String> {
    if width == 0 || height == 0 || rgba.len() != width * height * 4 {
        return Err("ukuran gambar tidak valid".into());
    }
    if width * height > 2_000_000 {
        return Err("gambar terlalu besar (maks ~2 MP)".into());
    }
    let colors = colors.clamp(2, 64);
    let pixels: Vec<[u8; 3]> = rgba
        .chunks_exact(4)
        .map(|p| {
            // Komposit ke putih bila ada alfa.
            let a = p[3] as u32;
            [
                ((p[0] as u32 * a + 255 * (255 - a)) / 255) as u8,
                ((p[1] as u32 * a + 255 * (255 - a)) / 255) as u8,
                ((p[2] as u32 * a + 255 * (255 - a)) / 255) as u8,
            ]
        })
        .collect();

    let palette = median_cut(&pixels, colors);
    if palette.is_empty() {
        return Err("palet kosong".into());
    }
    let mut quant = ColorImage::new_w_h(width, height);
    for (i, p) in pixels.iter().enumerate() {
        let q = nearest(*p, &palette);
        quant.pixels[i * 4] = q[0];
        quant.pixels[i * 4 + 1] = q[1];
        quant.pixels[i * 4 + 2] = q[2];
        quant.pixels[i * 4 + 3] = 255;
    }

    // detail 0..400 -> length_threshold 8.0..1.0, corner 120..30.
    // (v1.0: 0..100; v1.1: rentang 4x, ujungnya sama — detail 400 baru
    // = detail 100 lama, langkahnya lebih halus.)
    let t = detail.clamp(0.0, 400.0) / 400.0;
    let config = Config {
        color_mode: ColorMode::Color,
        hierarchical: Hierarchical::Stacked,
        filter_speckle: 4,
        color_precision: 8,
        layer_difference: 1,
        mode: visioncortex::PathSimplifyMode::Spline,
        corner_threshold: (120.0 - 90.0 * t) as i32,
        length_threshold: 8.0 - 7.0 * t,
        max_iterations: 10,
        splice_threshold: 45,
        path_precision: Some(2),
    };
    let mut svg = vtracer::convert(quant, config).map_err(|e| e.to_string())?;
    // Snap warna fill tiap path ke warna palet terdekat. vtracer memakai
    // residue_color() = rata-rata campuran piksel di cluster, sehingga
    // tanpa snap, output bisa punya warna jauh lebih banyak dari slider
    // (v1.0: slider 16 -> statistik "178 warna"). Setelah snap, "jumlah
    // warna" benar-benar dihormati: warna unik output <= slider.
    for p in svg.paths.iter_mut() {
        let q = nearest([p.color.r, p.color.g, p.color.b], &palette);
        p.color = visioncortex::Color::new(q[0], q[1], q[2]);
    }
    let text = svg.to_string();
    let mut fills: Vec<String> = svg
        .paths
        .iter()
        .map(|p| p.color.to_hex_string())
        .collect();
    fills.sort();
    fills.dedup();
    let out = serde_json::json!({
        "svg": text,
        "paths": svg.paths.len(),
        "colors": fills.len(),
        "anchors": count_anchors(&text),
    });
    Ok(out.to_string())
}

#[no_mangle]
pub extern "system" fn Java_com_hozinking_vector_VectorEngine_traceNative(
    env: JNIEnv,
    _obj: JObject,
    rgba: JByteArray,
    width: jint,
    height: jint,
    colors: jint,
    detail: jint,
) -> jstring {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let bytes = env.convert_byte_array(&rgba).map_err(|e| e.to_string())?;
        trace_inner(
            &bytes,
            width as usize,
            height as usize,
            colors as usize,
            detail as f64,
        )
    }));
    match result {
        Ok(Ok(json)) => match env.new_string(json) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        _ => std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, rgb: [u8; 3]) -> Vec<[u8; 3]> {
        vec![rgb; w * h]
    }

    #[test]
    fn median_cut_keeps_few_colors() {
        let mut px = solid(4, 4, [255, 0, 0]);
        px.extend(solid(4, 4, [0, 255, 0]));
        px.extend(solid(4, 4, [0, 0, 255]));
        let pal = median_cut(&px, 8);
        assert_eq!(pal.len(), 3);
        assert!(pal.contains(&[255, 0, 0]));
        assert!(pal.contains(&[0, 255, 0]));
        assert!(pal.contains(&[0, 0, 255]));
    }

    #[test]
    fn median_cut_reduces_gradient() {
        let mut px = Vec::new();
        for i in 0..256u16 {
            px.push([(i % 256) as u8, (i / 2) as u8, 128]);
        }
        let pal = median_cut(&px, 4);
        assert_eq!(pal.len(), 4);
    }

    #[test]
    fn trace_red_square_on_white() {
        // 64x64: latar putih + kotak merah di tengah.
        let (w, h) = (64usize, 64usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let red = (16..48).contains(&x) && (16..48).contains(&y);
                rgba[i] = if red { 220 } else { 255 };
                rgba[i + 1] = if red { 30 } else { 255 };
                rgba[i + 2] = if red { 30 } else { 255 };
                rgba[i + 3] = 255;
            }
        }
        let json = trace_inner(&rgba, w, h, 4, 50.0).expect("trace");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        let svg = v["svg"].as_str().unwrap();
        assert!(svg.contains("<svg"), "svg tidak berisi tag <svg>");
        assert!(svg.contains("<path"), "svg tidak berisi path");
        assert!(v["paths"].as_u64().unwrap() >= 1);
        assert!(v["colors"].as_u64().unwrap() >= 2, "kotak merah + latar harus >= 2 warna");
        assert!(v["anchors"].as_u64().unwrap() > 0);
    }

    #[test]
    fn trace_rejects_bad_input() {
        assert!(trace_inner(&[], 0, 0, 8, 50.0).is_err());
        assert!(trace_inner(&[0u8; 12], 1, 1, 8, 50.0).is_err());
    }

    /// Gambar "foto ramai" deterministik: gradien + noise + bentuk-bentuk
    /// berwarna (mirip screenshot aplikasi), supaya median-cut punya
    /// ratusan ribu warna berbeda seperti foto asli.
    fn busy_photo(w: usize, h: usize) -> Vec<u8> {
        let mut rgba = vec![0u8; w * h * 4];
        let mut s: u32 = 0x1234_5678;
        let mut rnd = move || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s >> 16) as u8
        };
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                // Latar gradien biru gelap.
                let mut r = (x * 40 / w) as u8;
                let mut g = (y * 60 / h) as u8;
                let mut b = 90u8 + ((x + y) * 40 / (w + h)) as u8;
                // Lingkaran putih di tengah.
                let (cx, cy) = (w as i32 / 2, h as i32 / 3);
                let dx = x as i32 - cx;
                let dy = y as i32 - cy;
                if dx * dx + dy * dy < (w as i32 / 6) * (w as i32 / 6) {
                    r = 245;
                    g = 245;
                    b = 245;
                }
                // Tombol-tombol warna di bawah.
                if y > h * 2 / 3 && y < h * 2 / 3 + h / 12 {
                    let band = x * 4 / w;
                    let (br, bg, bb) = match band {
                        0 => (20u8, 140u8, 120u8),
                        1 => (40u8, 160u8, 60u8),
                        2 => (200u8, 170u8, 30u8),
                        _ => (120u8, 60u8, 160u8),
                    };
                    r = br;
                    g = bg;
                    b = bb;
                }
                // Noise kecil supaya tiap piksel nyaris unik.
                let n = rnd() % 7;
                rgba[i] = r.saturating_add(n);
                rgba[i + 1] = g.saturating_add(n);
                rgba[i + 2] = b.saturating_add(n);
                rgba[i + 3] = 255;
            }
        }
        rgba
    }

    /// Regresi: warna unik di output TIDAK BOLEH melebihi slider.
    /// (Insiden v1.0: slider 16 tapi statistik "178 warna" — vtracer
    /// memakai residue_color() = rata-rata campuran piksel cluster,
    /// sehingga fill path bisa jadi warna baru di luar palet.)
    #[test]
    fn trace_output_colors_never_exceed_requested() {
        let (w, h) = (128usize, 128usize);
        let rgba = busy_photo(w, h);
        let json = trace_inner(&rgba, w, h, 8, 50.0).expect("trace");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        let colors = v["colors"].as_u64().unwrap();
        assert!(
            colors <= 8,
            "output punya {colors} warna padahal slider 8"
        );
    }

    /// Probe performa (di-ignore di CI): foto ramai 1024px pada setting
    /// tertinggi. Jalankan manual: cargo test -- --ignored --nocapture.
    #[test]
    #[ignore]
    fn trace_perf_busy_photo_64_400() {
        let (w, h) = (1024usize, 1024usize);
        let rgba = busy_photo(w, h);
        for (c, d) in [(16, 100.0), (64, 400.0)] {
            let t0 = std::time::Instant::now();
            let json = trace_inner(&rgba, w, h, c, d).expect("trace");
            let dt = t0.elapsed();
            let v: serde_json::Value = serde_json::from_str(&json).expect("json");
            println!(
                "PERF colors={c} detail={d}: {:.2?} | {} warna · {} anchor · {} path · {} byte",
                dt,
                v["colors"].as_u64().unwrap(),
                v["anchors"].as_u64().unwrap(),
                v["paths"].as_u64().unwrap(),
                v["svg"].as_str().unwrap().len(),
            );
        }
    }
}
