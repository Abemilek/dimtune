use std::path::PathBuf;

pub(crate) struct Config {
    pub(crate) folder: Option<PathBuf>,
    pub(crate) shuffle: bool,
    pub(crate) volume: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            folder: None,
            shuffle: false,
            volume: 0.8,
        }
    }
}

impl Config {
    pub(crate) fn load() -> Self {
        std::fs::read_to_string(config_path())
            .map(|text| Self::from_text(&text))
            .unwrap_or_default()
    }

    fn from_text(text: &str) -> Self {
        let mut config = Self::default();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "folder" if !value.trim().is_empty() => {
                        config.folder = Some(PathBuf::from(value.trim()))
                    }
                    "shuffle" => config.shuffle = value.trim() == "1",
                    "volume" => {
                        if let Ok(number) = value.trim().parse::<f32>()
                            && number.is_finite()
                        {
                            config.volume = number.clamp(0.0, 1.0);
                        }
                    }
                    _ => {}
                }
            }
        }
        config
    }

    pub(crate) fn save(&self) {
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let folder = self
            .folder
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = format!(
            "folder={}\nshuffle={}\nvolume={:.2}\n",
            folder,
            if self.shuffle { 1 } else { 0 },
            self.volume
        );
        // Writing to a temporary file and renaming avoids leaving a partial file.
        let temp = path.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&temp, text).is_ok() {
            if std::fs::rename(&temp, &path).is_err() {
                let _ = std::fs::remove_file(&temp);
            }
        } else {
            let _ = std::fs::remove_file(&temp);
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn config_base_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

fn config_path() -> PathBuf {
    config_base_dir().join("dimtune").join("config")
}

/// The user's XDG music folder, for example ~/Music or its localized name.
/// If there is no valid folder, fall back to ~/Music even if it does not exist:
/// scanning the whole home directory by default would be too expensive and surprising.
pub(crate) fn default_music_folder() -> PathBuf {
    let home_dir = home();
    let dirs = config_base_dir().join("user-dirs.dirs");
    if let Ok(text) = std::fs::read_to_string(dirs) {
        for line in text.lines() {
            if let Some(value) = line.trim().strip_prefix("XDG_MUSIC_DIR=") {
                let value = value
                    .trim()
                    .trim_matches('"')
                    .replace("$HOME", &home_dir.to_string_lossy());
                let path = PathBuf::from(value);
                if path.is_dir() && path != home_dir {
                    return path;
                }
            }
        }
    }
    home_dir.join("Music")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_from_text() {
        let config = Config::from_text("folder=/tmp/music\nshuffle=1\nvolume=1.7\nunknown=value\n");
        assert_eq!(config.folder, Some(PathBuf::from("/tmp/music")));
        assert!(config.shuffle);
        assert_eq!(config.volume, 1.0);

        let config = Config::from_text("volume=-0.5\n");
        assert_eq!(config.volume, 0.0);
        assert_eq!(config.folder, None);
        assert!(!config.shuffle);

        assert_eq!(Config::from_text("volume=NaN\n").volume, 0.8);
        assert_eq!(Config::from_text("folder=\n").folder, None);
    }
}
