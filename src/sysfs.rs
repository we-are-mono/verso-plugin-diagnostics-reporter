// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

//! Read access to sysfs, behind a trait so the sensor lookup runs against a
//! test tree with no device.

use std::fs;
use std::path::{Path, PathBuf};

pub trait SysFs {
    /// Contents of a file, trailing newlines removed. None if missing or unreadable.
    fn read(&self, path: &str) -> Option<String>;
    /// Canonical target of a path (like `readlink -f`). None if it does not resolve.
    fn canonical(&self, path: &str) -> Option<String>;
    /// Names in a directory that start with `prefix`, sorted bytewise (the
    /// order of a shell glob in the C locale). Empty if the directory is missing.
    fn list(&self, dir: &str, prefix: &str) -> Vec<String>;
}

/// DirSysFs reads absolute paths below `root`: "/" on the router, a temporary
/// tree in tests. Links in a test tree are relative, so they resolve inside it.
pub struct DirSysFs {
    root: PathBuf,
}

impl DirSysFs {
    pub fn new(root: impl Into<PathBuf>) -> DirSysFs {
        DirSysFs { root: root.into() }
    }

    /// The real sysfs.
    pub fn real() -> DirSysFs {
        DirSysFs::new("/")
    }

    fn at(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }
}

impl SysFs for DirSysFs {
    fn read(&self, path: &str) -> Option<String> {
        let text = fs::read_to_string(self.at(path)).ok()?;
        Some(text.trim_end_matches('\n').to_string())
    }

    fn canonical(&self, path: &str) -> Option<String> {
        let resolved = fs::canonicalize(self.at(path)).ok()?;
        let root = fs::canonicalize(&self.root).ok()?;
        let inside = resolved.strip_prefix(&root).ok()?;
        Some(Path::new("/").join(inside).to_string_lossy().into_owned())
    }

    fn list(&self, dir: &str, prefix: &str) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.at(dir)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| name.starts_with(prefix))
            .collect();
        names.sort();
        names
    }
}

/// Tree builds a sysfs-shaped directory for tests.
#[cfg(test)]
pub mod tree {
    use super::DirSysFs;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    pub struct Tree {
        pub root: PathBuf,
    }

    impl Tree {
        pub fn new() -> Tree {
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            let root = std::env::temp_dir()
                .join(format!("verso-plugin-diag-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Tree { root }
        }

        /// file writes `text` and a newline at the absolute `path`.
        pub fn file(&self, path: &str, text: &str) -> &Tree {
            let at = self.root.join(path.trim_start_matches('/'));
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            fs::write(at, format!("{text}\n")).unwrap();
            self
        }

        /// dir makes an empty directory at the absolute `path`.
        pub fn dir(&self, path: &str) -> &Tree {
            fs::create_dir_all(self.root.join(path.trim_start_matches('/'))).unwrap();
            self
        }

        /// link makes `path` a link to the absolute `target`, written relative so
        /// it resolves inside the tree.
        pub fn link(&self, path: &str, target: &str) -> &Tree {
            let at = self.root.join(path.trim_start_matches('/'));
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            let depth = Path::new(path.trim_start_matches('/')).components().count() - 1;
            let mut relative = PathBuf::new();
            for _ in 0..depth {
                relative.push("..");
            }
            relative.push(target.trim_start_matches('/'));
            symlink(relative, at).unwrap();
            self
        }

        pub fn fs(&self) -> DirSysFs {
            DirSysFs::new(&self.root)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tree::Tree;
    use super::SysFs;

    #[test]
    fn read_drops_the_trailing_newline() {
        let t = Tree::new();
        t.file("/sys/class/hwmon/hwmon0/name", "ddr_thermal");
        assert_eq!(t.fs().read("/sys/class/hwmon/hwmon0/name").as_deref(), Some("ddr_thermal"));
    }

    #[test]
    fn read_of_a_missing_file_is_none() {
        let t = Tree::new();
        assert_eq!(t.fs().read("/sys/class/hwmon/hwmon0/name"), None);
    }

    #[test]
    fn canonical_follows_links_inside_the_tree() {
        let t = Tree::new();
        t.dir("/sys/firmware/devicetree/base/soc/ethernet-phy@0");
        t.link(
            "/sys/class/hwmon/hwmon0/device/of_node",
            "/sys/firmware/devicetree/base/soc/ethernet-phy@0",
        );
        assert_eq!(
            t.fs().canonical("/sys/class/hwmon/hwmon0/device/of_node").as_deref(),
            Some("/sys/firmware/devicetree/base/soc/ethernet-phy@0")
        );
    }

    #[test]
    fn canonical_of_a_missing_path_is_none() {
        let t = Tree::new();
        t.dir("/sys/class/hwmon/hwmon6/device");
        assert_eq!(t.fs().canonical("/sys/class/hwmon/hwmon6/device/of_node"), None);
    }

    #[test]
    fn list_sorts_like_a_shell_glob() {
        let t = Tree::new();
        for n in ["hwmon2", "hwmon10", "hwmon1", "other"] {
            t.dir(&format!("/sys/class/hwmon/{n}"));
        }
        assert_eq!(t.fs().list("/sys/class/hwmon", "hwmon"), ["hwmon1", "hwmon10", "hwmon2"]);
    }

    #[test]
    fn list_of_a_missing_directory_is_empty() {
        let t = Tree::new();
        assert!(t.fs().list("/sys/class/hwmon", "hwmon").is_empty());
    }
}
