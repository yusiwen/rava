//! Spring Boot's nested-jar layout (M4.4).
//!
//! A Boot executable jar is one outer zip that carries the application classes under
//! `BOOT-INF/classes/` and its dependencies as whole jars under `BOOT-INF/lib/`. `java`
//! cannot name those nested jars in a classpath, so Boot ships its own launcher and
//! class loader. This module models only the *source* half of that layout: an ordered
//! [`ClassSource`] over the outer jar's classes root and its nested jars.
//!
//! The nested jars are indexed in **lexical entry-name order** (`BOOT-INF/lib/a.jar`
//! before `BOOT-INF/lib/z.jar`), not in central-directory order. [`JarSource`] already
//! indexes entries through a hash map, so lexical order is the stable order that does not
//! depend on how the jar was written; duplicate classes and resources therefore resolve
//! deterministically. The nested jars are read and indexed eagerly when the source is
//! built, so the outer jar and every dependency's bytes are held in memory; there is no
//! lazy or streaming nested-entry access.
//!
//! This is not Boot's loader. It does not implement `JarLauncher`, `LaunchedURLClassLoader`
//! or `URLClassLoader`, and it does not support `loader.path`, `JarLauncher`'s exec
//! behaviour or a nested jar's own manifest `Class-Path`. Classes still get the ordinary
//! `(loader, name)` identity of the source that supplied them, and resources are read
//! through the same [`ClassSource`] methods as every other source.

use std::path::{Path, PathBuf};

use crate::VmError;
use crate::source::{ClassSource, JarSource, absolute_path, percent_encode_path};

/// The application classes root inside the outer jar.
pub const CLASSES_PREFIX: &str = "BOOT-INF/classes/";

/// The nested dependency jars prefix inside the outer jar.
pub const LIB_PREFIX: &str = "BOOT-INF/lib/";

/// One nested `BOOT-INF/lib/*.jar` and the entry name that carries it.
#[derive(Debug)]
struct NestedJar {
    /// The exact outer-jar entry, for example `BOOT-INF/lib/demo-helper.jar`.
    entry: String,
    source: JarSource,
}

/// A Spring Boot executable jar read as a [`ClassSource`].
///
/// Use [`BootJarSource::is_boot`] or [`BootJarSource::from_jar`] to tell a Boot layout
/// from a flat jar: a jar with no `BOOT-INF/classes/` or `BOOT-INF/lib/` entry is not a
/// Boot jar, and `from_jar` returns `None` for it.
#[derive(Debug)]
pub struct BootJarSource {
    outer: JarSource,
    /// The outer jar's absolute path, used to build the nested external URL forms.
    outer_path: PathBuf,
    /// Nested jars in lexical entry-name order.
    libraries: Vec<NestedJar>,
}

impl BootJarSource {
    /// Whether `jar` carries a Boot layout (`BOOT-INF/classes/` or `BOOT-INF/lib/`).
    pub fn is_boot(jar: &JarSource) -> bool {
        jar.entry_names()
            .into_iter()
            .any(|entry| entry.starts_with(CLASSES_PREFIX) || entry.starts_with(LIB_PREFIX))
    }

    /// Open `path`, returning `None` when it is a jar without a Boot layout.
    pub fn open(path: impl Into<PathBuf>) -> Result<Option<Self>, VmError> {
        Self::from_jar(JarSource::open(path)?)
    }

    /// Index an already-opened jar, returning `None` when it has no Boot layout.
    ///
    /// A `BOOT-INF/lib/*.jar` entry that is not a usable zip fails with
    /// [`VmError::BootEntry`] naming the entry; a jar with no Boot entry at all is simply
    /// `None`, because it is an ordinary flat jar rather than a malformed Boot one.
    pub fn from_jar(outer: JarSource) -> Result<Option<Self>, VmError> {
        if !Self::is_boot(&outer) {
            return Ok(None);
        }
        let outer_path = absolute_path(outer.path());
        let mut libraries = Vec::new();
        for entry in outer.entry_names() {
            let Some(file_name) = entry.strip_prefix(LIB_PREFIX) else {
                continue;
            };
            // `BOOT-INF/lib/*.jar` means the direct children only; a deeper path is not a
            // Boot dependency and is left alone.
            if file_name.contains('/') || !file_name.ends_with(".jar") {
                continue;
            }
            let bytes = outer.read_entry(entry)?.ok_or_else(|| VmError::BootEntry {
                entry: entry.to_owned(),
                detail: "the entry is not readable".to_owned(),
            })?;
            let source = JarSource::from_bytes(entry.to_owned(), bytes).map_err(|error| {
                VmError::BootEntry {
                    entry: entry.to_owned(),
                    detail: error.to_string(),
                }
            })?;
            libraries.push(NestedJar {
                entry: entry.to_owned(),
                source,
            });
        }
        Ok(Some(Self {
            outer,
            outer_path,
            libraries,
        }))
    }

    /// The outer jar's path.
    pub fn path(&self) -> &Path {
        self.outer.path()
    }

    /// The nested jars in lookup order, as their exact outer-jar entry names.
    pub fn library_entry_names(&self) -> Vec<&str> {
        self.libraries
            .iter()
            .map(|nested| nested.entry.as_str())
            .collect()
    }

    /// Read one exact entry under `BOOT-INF/classes/`.
    ///
    /// A nested URL names one root explicitly, so this method never falls back to a
    /// nested jar; [`ClassSource::read_resource`] is the searching view.
    pub fn read_classes_entry(&self, name: &str) -> Result<Option<Vec<u8>>, VmError> {
        self.outer.read_entry(&classes_entry(name))
    }

    /// Read one exact entry from the named nested jar (its full `BOOT-INF/lib/...` entry).
    pub fn read_library_entry(
        &self,
        library: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>, VmError> {
        let nested = self
            .libraries
            .iter()
            .find(|nested| nested.entry == library)
            .ok_or_else(|| VmError::Jar(format!("the Boot layout has no nested jar {library}")))?;
        nested.source.read_entry(name)
    }
}

impl ClassSource for BootJarSource {
    /// `BOOT-INF/classes/<binary>.class` first, then each nested jar in lexical order.
    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError> {
        let relative = format!("{}.class", binary_name.replace('.', "/"));
        if let Some(bytes) = self.read_classes_entry(&relative)? {
            return Ok(Some(bytes));
        }
        for nested in &self.libraries {
            if let Some(bytes) = nested.source.read(binary_name)? {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    /// `BOOT-INF/classes/<name>` first, then each nested jar in lexical order.
    fn read_resource(&self, name: &str) -> Result<Option<Vec<u8>>, VmError> {
        if let Some(bytes) = self.read_classes_entry(name)? {
            return Ok(Some(bytes));
        }
        for nested in &self.libraries {
            if let Some(bytes) = nested.source.read_resource(name)? {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    /// Every copy, classes root first and then each nested jar in lookup order.
    fn read_resources(&self, name: &str) -> Result<Vec<Vec<u8>>, VmError> {
        let mut resources = Vec::new();
        if let Some(bytes) = self.read_classes_entry(name)? {
            resources.push(bytes);
        }
        for nested in &self.libraries {
            resources.extend(nested.source.read_resources(name)?);
        }
        Ok(resources)
    }

    /// Boot-style nested external forms for every copy of `name`.
    ///
    /// The classes root is `jar:file:<outer-abs>!/BOOT-INF/classes!/<name>` and a nested
    /// jar is `jar:file:<outer-abs>!/BOOT-INF/lib/<lib>.jar!/<name>`, with path components
    /// percent-encoded exactly as [`JarSource`] encodes a flat jar URL.
    fn resource_urls(&self, name: &str) -> Result<Vec<String>, VmError> {
        let outer = percent_encode_path(&self.outer_path.to_string_lossy());
        let entry = percent_encode_path(name);
        let mut urls = Vec::new();
        if self.outer.has_entry(&classes_entry(name)) {
            urls.push(format!("jar:file:{outer}!/BOOT-INF/classes!/{entry}"));
        }
        for nested in &self.libraries {
            if nested.source.has_entry(name) {
                let library = percent_encode_path(&nested.entry);
                urls.push(format!("jar:file:{outer}!/{library}!/{entry}"));
            }
        }
        Ok(urls)
    }

    /// Resource names with `BOOT-INF/classes/` stripped, then every nested jar's names.
    fn resource_names(&self, prefix: &str) -> Result<Vec<String>, VmError> {
        let mut names = Vec::new();
        for entry in self.outer.entry_names() {
            if let Some(relative) = entry.strip_prefix(CLASSES_PREFIX) {
                if relative.starts_with(prefix) {
                    names.push(relative.to_owned());
                }
            }
        }
        for nested in &self.libraries {
            names.extend(nested.source.resource_names(prefix)?);
        }
        Ok(names)
    }
}

/// The outer-jar entry that holds `name` under the classes root.
fn classes_entry(name: &str) -> String {
    format!("{CLASSES_PREFIX}{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rava-boot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Build a stored (uncompressed) zip. The jar reader checks structure and sizes, not
    /// the checksum, so a zero CRC is fine.
    fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let size = data.len() as u32;
            let name_len = name.len() as u16;

            out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // stored
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            directory.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
            directory.extend_from_slice(&20u16.to_le_bytes());
            directory.extend_from_slice(&20u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes()); // stored
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u32.to_le_bytes());
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&name_len.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u32.to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }

        let directory_offset = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn boot_jar(libraries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("BOOT-INF/classes/demo/App.class", b"classes-app"),
            ("BOOT-INF/classes/demo/Shadowed.class", b"classes-shadowed"),
            ("BOOT-INF/classes/outer.txt", b"outer"),
        ];
        entries.extend_from_slice(libraries);
        stored_zip(&entries)
    }

    fn source(outer: Vec<u8>) -> BootJarSource {
        BootJarSource::from_jar(JarSource::from_bytes("outer.jar", outer).unwrap())
            .unwrap()
            .expect("a Boot layout")
    }

    #[test]
    fn is_boot_separates_a_boot_layout_from_a_flat_jar() {
        let flat =
            JarSource::from_bytes("flat.jar", stored_zip(&[("demo/App.class", b"app")])).unwrap();
        assert!(!BootJarSource::is_boot(&flat));
        assert!(BootJarSource::from_jar(flat).unwrap().is_none());

        let boot = JarSource::from_bytes("boot.jar", boot_jar(&[])).unwrap();
        assert!(BootJarSource::is_boot(&boot));
        assert!(BootJarSource::from_jar(boot).unwrap().is_some());
    }

    #[test]
    fn reads_classes_first_then_nested_jars_in_lexical_order() {
        let a = stored_zip(&[("demo/OnlyA.class", b"a-only-a"), ("inner.txt", b"inner-a")]);
        let z = stored_zip(&[("demo/OnlyZ.class", b"z-only-z"), ("inner.txt", b"inner-z")]);
        let source = source(boot_jar(&[
            ("BOOT-INF/lib/z.jar", &z),
            ("BOOT-INF/lib/a.jar", &a),
        ]));

        assert_eq!(
            source.library_entry_names(),
            vec!["BOOT-INF/lib/a.jar", "BOOT-INF/lib/z.jar"]
        );
        assert_eq!(
            source.read("demo.App").unwrap().as_deref(),
            Some(&b"classes-app"[..])
        );
        assert_eq!(
            source.read("demo.OnlyA").unwrap().as_deref(),
            Some(&b"a-only-a"[..])
        );
        assert_eq!(
            source.read("demo.OnlyZ").unwrap().as_deref(),
            Some(&b"z-only-z"[..])
        );
        assert_eq!(source.read("demo.Missing").unwrap(), None);

        // The classes root wins and the nested jars are searched in lexical order.
        assert_eq!(
            source.read_resource("inner.txt").unwrap().as_deref(),
            Some(&b"inner-a"[..])
        );
        assert_eq!(
            source.read_resources("inner.txt").unwrap(),
            vec![b"inner-a".to_vec(), b"inner-z".to_vec()]
        );
        assert_eq!(
            source.read_resources("outer.txt").unwrap(),
            vec![b"outer".to_vec()]
        );
        assert!(source.read_resources("missing.txt").unwrap().is_empty());
    }

    #[test]
    fn resource_names_strip_the_classes_prefix_and_combine_nested_names() {
        let a = stored_zip(&[
            ("demo/OnlyA.class", b"a"),
            ("META-INF/services/demo.Service", b"demo.Provider"),
        ]);
        let source = source(boot_jar(&[("BOOT-INF/lib/a.jar", &a)]));

        assert_eq!(
            source.resource_names("demo/").unwrap(),
            vec!["demo/App.class", "demo/Shadowed.class", "demo/OnlyA.class"]
        );
        assert_eq!(
            source.resource_names("META-INF/").unwrap(),
            vec!["META-INF/services/demo.Service"]
        );
    }

    #[test]
    fn resource_urls_use_the_boot_nested_external_forms() {
        let dir = scratch("urls");
        let a = stored_zip(&[("inner.txt", b"inner")]);
        let path = dir.join("app.jar");
        std::fs::write(&path, boot_jar(&[("BOOT-INF/lib/demo-helper.jar", &a)])).unwrap();
        let source = BootJarSource::open(&path).unwrap().expect("a Boot layout");
        let outer = percent_encode_path(&absolute_path(&path).to_string_lossy());

        assert_eq!(
            source.resource_urls("outer.txt").unwrap(),
            vec![format!("jar:file:{outer}!/BOOT-INF/classes!/outer.txt")]
        );
        assert_eq!(
            source.resource_urls("inner.txt").unwrap(),
            vec![format!(
                "jar:file:{outer}!/BOOT-INF/lib/demo-helper.jar!/inner.txt"
            )]
        );
        assert!(source.resource_urls("missing.txt").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_malformed_nested_jar_names_its_entry() {
        let outer = boot_jar(&[("BOOT-INF/lib/broken.jar", b"not a zip archive")]);
        let error = BootJarSource::from_jar(JarSource::from_bytes("outer.jar", outer).unwrap())
            .expect_err("the nested entry is not a jar");
        let message = error.to_string();
        assert!(
            message.contains("BOOT-INF/lib/broken.jar"),
            "message: {message}"
        );
        assert!(
            matches!(error, VmError::BootEntry { ref entry, .. } if entry == "BOOT-INF/lib/broken.jar")
        );
    }
}
