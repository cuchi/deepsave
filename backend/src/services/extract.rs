use anyhow::{anyhow, Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, Luma};
use std::path::PathBuf;

/// Extract raw text from a document file.
///
/// - PDFs: embedded text layer via `pdf-extract`.
/// - Images: OCR via the `tesseract` CLI (Portuguese + English) after preprocessing.
///
/// Scanned PDFs (no text layer) and AI structuring are handled in later milestones.
pub async fn extract_raw_text(file_path: &str, content_type: &str) -> Result<String> {
    let lower = file_path.to_lowercase();
    if lower.ends_with(".pdf") {
        let path = file_path.to_string();
        tokio::task::spawn_blocking(move || pdf_text(&path))
            .await
            .context("pdf extraction task panicked")?
    } else if is_image(file_path, content_type) {
        ocr_text(file_path).await
    } else {
        Ok(String::new())
    }
}

fn is_image(file_path: &str, content_type: &str) -> bool {
    let lower = file_path.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || content_type.starts_with("image/")
}

fn pdf_text(path: &str) -> Result<String> {
    pdf_extract::extract_text(path)
        .map(|t| t.trim().to_string())
        .map_err(|e| anyhow!("pdf extraction failed: {e}"))
}

async fn ocr_text(path: &str) -> Result<String> {
    let preprocessed = preprocess_image(path).await?;

    let result = tokio::process::Command::new("tesseract")
        .arg(&preprocessed)
        .arg("stdout")
        .arg("-l")
        .arg("por+eng")
        .arg("--oem")
        .arg("1")
        .arg("--psm")
        .arg("6")
        .arg("--dpi")
        .arg("300")
        .output()
        .await
        .context("failed to run tesseract")?;

    let _ = tokio::fs::remove_file(&preprocessed).await;

    if !result.status.success() {
        return Err(anyhow!(
            "tesseract failed: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
}

/// Preprocess an image for OCR: upscale if small, grayscale, then Otsu binarize.
/// Returns the path to a temporary PNG.
async fn preprocess_image(path: &str) -> Result<PathBuf> {
    let src = path.to_string();
    let tmp = format!("{path}.ocr.png");
    let out = tmp.clone();
    tokio::task::spawn_blocking(move || {
        let img = image::open(&src).context("failed to open image")?;
        let (w, h) = (img.width(), img.height());
        let min_dim = w.min(h);
        let scale = if min_dim < 800 {
            3.0
        } else if min_dim < 1600 {
            2.0
        } else {
            1.0
        };
        let img: DynamicImage = if scale > 1.0 {
            img.resize(
                (w as f32 * scale) as u32,
                (h as f32 * scale) as u32,
                FilterType::Lanczos3,
            )
        } else {
            img
        };
        let gray = img.grayscale().to_luma8();
        let binary = otsu_binarize(&gray);
        binary.save(&out).context("failed to save preprocessed image")?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("image preprocessing panicked")??;
    Ok(PathBuf::from(tmp))
}

/// Global Otsu threshold: produce dark text (0) on white background (255).
fn otsu_binarize(img: &GrayImage) -> GrayImage {
    let mut hist = [0u64; 256];
    for p in img.pixels() {
        hist[p[0] as usize] += 1;
    }
    let total = (img.width() as u64) * (img.height() as u64);

    let mut sum = 0u64;
    for (i, &c) in hist.iter().enumerate() {
        sum += i as u64 * c;
    }

    let mut sum_b = 0u64;
    let mut w_b = 0u64;
    let mut max_var = 0.0f64;
    let mut threshold = 127u8;

    for (t, &c) in hist.iter().enumerate() {
        w_b += c;
        if w_b == 0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0 {
            break;
        }
        sum_b += t as u64 * c;
        let m_b = sum_b as f64 / w_b as f64;
        let m_f = (sum - sum_b) as f64 / w_f as f64;
        let var = (w_b as f64) * (w_f as f64) * (m_b - m_f) * (m_b - m_f);
        if var > max_var {
            max_var = var;
            threshold = t as u8;
        }
    }

    let mut out = GrayImage::new(img.width(), img.height());
    for (x, y, p) in img.enumerate_pixels() {
        let v = if p[0] <= threshold { 0 } else { 255 };
        out.put_pixel(x, y, Luma([v]));
    }
    out
}
