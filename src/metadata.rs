use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::ColorImage;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;

pub(crate) struct Metadata {
    pub(crate) generation: u64,
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    pub(crate) duration: Option<Duration>,
    pub(crate) cover: Option<ColorImage>,
}

fn decode_cover(bytes: &[u8]) -> Option<ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = image.thumbnail(320, 320).to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
}

fn folder_cover(file: &Path) -> Option<ColorImage> {
    let dir = file.parent()?;
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    for name in ["cover", "folder", "front", "album"] {
        for extension in ["jpg", "jpeg", "png"] {
            for path in &entries {
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !stem.eq_ignore_ascii_case(name) || !ext.eq_ignore_ascii_case(extension) {
                    continue;
                }
                if let Some(image) = std::fs::read(path)
                    .ok()
                    .and_then(|bytes| decode_cover(&bytes))
                {
                    return Some(image);
                }
            }
        }
    }
    None
}

pub(crate) fn read_metadata(generation: u64, path: &Path) -> Metadata {
    let mut metadata = Metadata {
        generation,
        title: None,
        artist: None,
        album: None,
        duration: None,
        cover: None,
    };
    if let Ok(tagged_file) = lofty::read_from_path(path) {
        let duration = tagged_file.properties().duration();
        if !duration.is_zero() {
            metadata.duration = Some(duration);
        }
        if let Some(tag) = tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag())
        {
            metadata.title = tag
                .title()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            metadata.artist = tag
                .artist()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            metadata.album = tag
                .album()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            metadata.cover = tag
                .pictures()
                .iter()
                .find_map(|picture| decode_cover(picture.data()));
        }
    }
    if metadata.cover.is_none() {
        metadata.cover = folder_cover(path);
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::temp_dir;
    use std::fs;

    #[test]
    fn folder_cover_is_case_insensitive() {
        let base = temp_dir();
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        fs::write(base.join("COVER.PNG"), bytes).unwrap();

        let cover = folder_cover(&base.join("song.mp3")).unwrap();
        assert!(cover.size[0] > 0 && cover.size[1] > 0);

        fs::remove_dir_all(&base).unwrap();
    }
}
