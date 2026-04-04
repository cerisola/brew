use crate::BrewResult;
use crate::utils::formatter::ohai;
use anyhow::{Context, anyhow};
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub(crate) fn cache_api_path() -> BrewResult<PathBuf> {
    Ok(env_path("HOMEBREW_CACHE")?.join("api"))
}

pub(crate) fn cache_path() -> BrewResult<PathBuf> {
    env_path("HOMEBREW_CACHE")
}

pub(crate) fn cellar_path() -> BrewResult<PathBuf> {
    env_path("HOMEBREW_CELLAR")
}

pub(crate) fn caskroom_path() -> BrewResult<PathBuf> {
    env_path("HOMEBREW_CASKROOM")
}

pub(crate) fn prefix_path() -> BrewResult<PathBuf> {
    env_path("HOMEBREW_PREFIX")
}

pub(crate) fn brew_file() -> BrewResult<PathBuf> {
    env_path("HOMEBREW_BREW_FILE")
}

pub(crate) fn brew_no_color() -> bool {
    env_bool("HOMEBREW_NO_COLOR")
}

pub(crate) fn brew_color() -> bool {
    env_bool("HOMEBREW_COLOR")
}

pub(crate) fn read_lines(path: &Path) -> BrewResult<Vec<String>> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect())
}

pub(crate) fn installed_names(path: &Path) -> BrewResult<Vec<String>> {
    list_directories(path)
}

pub(crate) fn installed_versions(path: &Path) -> BrewResult<Vec<String>> {
    list_directories(path)
}

pub(crate) fn list_files(path: &Path) -> BrewResult<Vec<PathBuf>> {
    if !path.exists() {
        return Err(anyhow!("Failed to list {}", path.display()));
    }

    let mut files = WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

pub(crate) fn print_sections(formulae: &[String], casks: &[String]) {
    let stdout_is_tty = io::stdout().is_terminal();

    if stdout_is_tty && !formulae.is_empty() && !casks.is_empty() {
        ohai("Formulae");
        println!("{}", formulae.join("\n"));
        println!();
        ohai("Casks");
        println!("{}", casks.join("\n"));
        return;
    }

    if !formulae.is_empty() {
        println!("{}", formulae.join("\n"));
    }
    if !formulae.is_empty() && !casks.is_empty() {
        println!();
    }
    if !casks.is_empty() {
        println!("{}", casks.join("\n"));
    }
}

fn env_path(name: &str) -> BrewResult<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("{name} is not set"))
}

fn env_bool(name: &str) -> bool {
    env::var_os(name)
        .map(|string| {
            if string.is_empty() {
                return false;
            }

            if let Some(string) = string.to_str() {
                return !matches!(
                    string.trim().to_lowercase().as_str(),
                    "0" | "false" | "off" | "no" | "nil"
                );
            }

            false
        })
        .unwrap_or(false)
}

fn list_directories(path: &Path) -> BrewResult<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut entries = fs::read_dir(path)
        .with_context(|| format!("Failed to list {}", path.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to list {}", path.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    Ok(entries
        .into_iter()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{list_files, read_lines};
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn trims_and_filters_empty_lines() {
        let tempdir = TestDir::new_in(&env::temp_dir());
        let path = tempdir.path().join("names.txt");
        fs::write(&path, "foo\n\n bar \n").unwrap();

        assert_eq!(
            read_lines(&path).unwrap(),
            vec!["foo".to_string(), "bar".to_string()]
        );
    }

    #[test]
    fn lists_files_in_sorted_order() {
        let tempdir = TestDir::new_in(&env::temp_dir());
        let root = tempdir.path();

        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("b.txt"), "").unwrap();
        fs::write(root.join("nested/a.txt"), "").unwrap();

        let files = list_files(root).unwrap();
        let files = files
            .iter()
            .map(|path| path.strip_prefix(root).unwrap().display().to_string())
            .collect::<Vec<_>>();

        assert_eq!(files, vec!["b.txt".to_string(), "nested/a.txt".to_string()]);
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new_in(root: &Path) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let path = root.join(format!(
                "brew-rs-global-{}-{}-{}",
                process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
