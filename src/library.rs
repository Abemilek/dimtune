use std::path::{Path, PathBuf};

const EXTENSIONS: [&str; 5] = ["mp3", "flac", "ogg", "wav", "m4a"];

pub(crate) struct ScanResult {
    pub(crate) generation: u64,
    pub(crate) folder: PathBuf,
    pub(crate) tracks: Vec<PathBuf>,
}

pub(crate) fn scan(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 12 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            scan(&path, out, depth + 1);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        {
            out.push(path);
        }
    }
}

pub(crate) fn track_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::temp_dir;
    use std::fs;

    #[test]
    fn name_without_extension() {
        assert_eq!(track_name(Path::new("/music/song.mp3")), "song");
    }

    #[test]
    fn recursive_scan_and_extensions() {
        let base = temp_dir();
        let sub = base.join("sub");
        let deep = sub.join("more");
        fs::create_dir_all(&deep).unwrap();
        for name in ["a.mp3", "B.FLAC", "note.txt"] {
            fs::write(base.join(name), b"").unwrap();
        }
        fs::write(sub.join("c.ogg"), b"").unwrap();
        fs::write(deep.join("d.wav"), b"").unwrap();
        fs::write(deep.join("e.opus"), b"").unwrap();

        let mut tracks = Vec::new();
        scan(&base, &mut tracks, 0);
        let names: Vec<String> = tracks
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(tracks.len(), 4);
        for name in ["a.mp3", "B.FLAC", "c.ogg", "d.wav"] {
            assert!(names.iter().any(|n| n == name), "missing {name}");
        }

        fs::remove_dir_all(&base).unwrap();
    }
}
