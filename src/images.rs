//! Saving uploaded recipe photos.
//!
//! iPhone photos are often HEIC, which the AI services (and most browsers)
//! can't read, so on a Mac they're converted to JPEG with the built-in `sips`
//! tool. Very large photos are shrunk the same way so uploads stay quick.

use std::path::Path;

use anyhow::{Context, Result, bail};

const SHRINK_ABOVE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SIDE_PX: &str = "2400";

pub struct Stored {
    /// File name inside the images folder.
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

fn kind(file_name: &str, content_type: &str) -> Option<&'static str> {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_lowercase();
    let ct = content_type.to_lowercase();
    Some(match (ext.as_str(), ct.as_str()) {
        ("jpg" | "jpeg", _) | (_, "image/jpeg") => "jpg",
        ("png", _) | (_, "image/png") => "png",
        ("webp", _) | (_, "image/webp") => "webp",
        ("gif", _) | (_, "image/gif") => "gif",
        ("heic" | "heif", _) | (_, "image/heic" | "image/heif") => "heic",
        _ => return None,
    })
}

fn mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/jpeg",
    }
}

pub async fn store(dir: &Path, file_name: &str, content_type: &str, bytes: &[u8]) -> Result<Stored> {
    let Some(ext) = kind(file_name, content_type) else {
        bail!("\"{file_name}\" isn't a photo this app can read. Use JPEG, PNG, WebP or HEIC.");
    };
    let id = uuid::Uuid::new_v4().simple().to_string();

    if ext == "heic" || bytes.len() > SHRINK_ABOVE_BYTES {
        match to_jpeg(dir, &id, ext, bytes).await {
            Ok(jpeg) => {
                let name = format!("{id}.jpg");
                tokio::fs::write(dir.join(&name), &jpeg).await?;
                return Ok(Stored {
                    name,
                    mime: "image/jpeg".into(),
                    bytes: jpeg,
                });
            }
            Err(e) if ext == "heic" => {
                return Err(e.context(format!(
                    "\"{file_name}\" is a HEIC photo and couldn't be converted. Export it as JPEG and try again."
                )));
            }
            Err(_) => {} // just keep the large original
        }
    }
    let name = format!("{id}.{ext}");
    tokio::fs::write(dir.join(&name), bytes)
        .await
        .context("saving the photo")?;
    Ok(Stored {
        name,
        mime: mime(ext).into(),
        bytes: bytes.to_vec(),
    })
}

/// Convert / shrink with macOS's `sips`.
async fn to_jpeg(dir: &Path, id: &str, ext: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let input = dir.join(format!("{id}.upload.{ext}"));
    let output = dir.join(format!("{id}.converted.jpg"));
    tokio::fs::write(&input, bytes).await?;
    let result = tokio::process::Command::new("sips")
        .args(["-s", "format", "jpeg", "-s", "formatOptions", "85", "-Z", MAX_SIDE_PX])
        .arg(&input)
        .arg("--out")
        .arg(&output)
        .output()
        .await;
    let _ = tokio::fs::remove_file(&input).await;
    let out = result.context("couldn't run the macOS `sips` tool")?;
    if !out.status.success() {
        let _ = tokio::fs::remove_file(&output).await;
        bail!("sips: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let jpeg = tokio::fs::read(&output).await?;
    let _ = tokio::fs::remove_file(&output).await;
    Ok(jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_photo_types() {
        assert_eq!(kind("card.JPG", ""), Some("jpg"));
        assert_eq!(kind("blob", "image/png"), Some("png"));
        assert_eq!(kind("IMG_1234.HEIC", "application/octet-stream"), Some("heic"));
        assert_eq!(kind("notes.pdf", "application/pdf"), None);
    }

    #[tokio::test]
    async fn stores_small_jpeg_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), "a.jpg", "image/jpeg", b"fake").await.unwrap();
        assert!(s.name.ends_with(".jpg"));
        assert_eq!(std::fs::read(dir.path().join(&s.name)).unwrap(), b"fake");
        assert!(store(dir.path(), "a.txt", "text/plain", b"x").await.is_err());
    }
}
