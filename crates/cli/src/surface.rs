//! The committed surface manifest: record types, counts and a strict parser.
//!
//! M1.2 renders this shape from the pinned jars. M1.3 consumes the committed text to
//! generate the stub library and to link-check it. One parser means the producer and
//! the consumers agree on what a record is.

use std::collections::BTreeSet;
use std::fmt;

/// One `owner name descriptor` record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Member {
    pub(crate) owner: String,
    pub(crate) name: String,
    pub(crate) descriptor: String,
}

impl Member {
    /// Renders the record as it appears in the committed manifest.
    pub(crate) fn render(&self, kind: &str) -> String {
        format!("{kind} {} {} {}\n", self.owner, self.name, self.descriptor)
    }
}

/// The record counts reported in the manifest header and on the command line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub classes: usize,
    pub fields: usize,
    pub methods: usize,
    pub interface_methods: usize,
}

/// The deduplicated platform surface of the pinned jar set.
#[derive(Debug, Default)]
pub struct Surface {
    pub(crate) classes: BTreeSet<String>,
    pub(crate) fields: BTreeSet<Member>,
    pub(crate) methods: BTreeSet<Member>,
    pub(crate) interface_methods: BTreeSet<Member>,
}

impl Surface {
    pub fn counts(&self) -> Counts {
        Counts {
            classes: self.classes.len(),
            fields: self.fields.len(),
            methods: self.methods.len(),
            interface_methods: self.interface_methods.len(),
        }
    }

    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.classes.iter().map(String::as_str)
    }

    pub fn fields(&self) -> impl Iterator<Item = &Member> {
        self.fields.iter()
    }

    pub fn methods(&self) -> impl Iterator<Item = &Member> {
        self.methods.iter()
    }

    pub fn interface_methods(&self) -> impl Iterator<Item = &Member> {
        self.interface_methods.iter()
    }

    /// Parses the committed manifest.
    ///
    /// Blank lines and `#` comments are allowed. A commented count such as
    /// `# classes: 980` is checked against the records; a stale header is a hard error,
    /// because it means the committed file and its report disagree.
    pub fn parse(text: &str) -> Result<Self, SurfaceError> {
        let mut surface = Self::default();
        let mut declared = DeclaredCounts::default();

        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(comment) = line.strip_prefix('#') {
                declared.record(comment.trim(), number)?;
                continue;
            }

            let mut parts = line.split_whitespace();
            let kind = parts.next().unwrap_or_default();
            match kind {
                "class" => {
                    let name = record_field(&mut parts, "class", "name", number)?;
                    no_more_fields(&mut parts, "class", number)?;
                    surface.classes.insert(name.to_owned());
                }
                "field" | "method" | "interface-method" => {
                    let owner = record_field(&mut parts, kind, "owner", number)?;
                    let name = record_field(&mut parts, kind, "name", number)?;
                    let descriptor = record_field(&mut parts, kind, "descriptor", number)?;
                    no_more_fields(&mut parts, kind, number)?;
                    let member = Member {
                        owner: owner.to_owned(),
                        name: name.to_owned(),
                        descriptor: descriptor.to_owned(),
                    };
                    match kind {
                        "field" => {
                            surface.fields.insert(member);
                        }
                        "method" => {
                            surface.methods.insert(member);
                        }
                        _ => {
                            surface.interface_methods.insert(member);
                        }
                    }
                }
                other => {
                    return Err(SurfaceError::new(format!(
                        "manifest line {number}: unknown record kind `{other}`"
                    )));
                }
            }
        }

        declared.verify(&surface.counts())?;
        Ok(surface)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceError(String);

impl SurfaceError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn record_field<'a>(
    parts: &mut impl Iterator<Item = &'a str>,
    kind: &str,
    field: &str,
    line: usize,
) -> Result<&'a str, SurfaceError> {
    parts.next().ok_or_else(|| {
        SurfaceError::new(format!(
            "manifest line {line}: {kind} record is missing its {field}"
        ))
    })
}

fn no_more_fields<'a>(
    parts: &mut impl Iterator<Item = &'a str>,
    kind: &str,
    line: usize,
) -> Result<(), SurfaceError> {
    if let Some(extra) = parts.next() {
        return Err(SurfaceError::new(format!(
            "manifest line {line}: unexpected field `{extra}` in a {kind} record"
        )));
    }
    Ok(())
}

/// The optional counts in the manifest header.
#[derive(Debug, Default)]
struct DeclaredCounts {
    classes: Option<usize>,
    fields: Option<usize>,
    methods: Option<usize>,
    interface_methods: Option<usize>,
}

impl DeclaredCounts {
    fn record(&mut self, comment: &str, line: usize) -> Result<(), SurfaceError> {
        let (label, value) = match comment.split_once(':') {
            Some((label, value)) => (label.trim(), value.trim()),
            None => return Ok(()),
        };
        let slot = match label {
            "classes" => &mut self.classes,
            "fields" => &mut self.fields,
            "methods" => &mut self.methods,
            "interface-methods" => &mut self.interface_methods,
            _ => return Ok(()),
        };
        let value = value.parse::<usize>().map_err(|_| {
            SurfaceError::new(format!(
                "manifest line {line}: `{label}` must be a non-negative integer, got `{value}`"
            ))
        })?;
        *slot = Some(value);
        Ok(())
    }

    fn verify(&self, counts: &Counts) -> Result<(), SurfaceError> {
        for (label, declared, actual) in [
            ("classes", self.classes, counts.classes),
            ("fields", self.fields, counts.fields),
            ("methods", self.methods, counts.methods),
            (
                "interface-methods",
                self.interface_methods,
                counts.interface_methods,
            ),
        ] {
            if let Some(declared) = declared {
                if declared != actual {
                    return Err(SurfaceError::new(format!(
                        "manifest header declares {declared} {label} but the records contain {actual}"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "\
# classes: 2
# fields: 1
# methods: 1
# interface-methods: 1
class java/lang/Object
class java/lang/Runnable
field java/lang/System out Ljava/io/PrintStream;
method java/lang/Object <init> ()V
interface-method java/lang/Runnable run ()V
";

    #[test]
    fn parses_records_and_checks_the_header_counts() {
        let surface = Surface::parse(MANIFEST).expect("manifest parses");
        assert_eq!(
            surface.counts(),
            Counts {
                classes: 2,
                fields: 1,
                methods: 1,
                interface_methods: 1,
            }
        );
        assert_eq!(
            surface.classes().collect::<Vec<_>>(),
            ["java/lang/Object", "java/lang/Runnable"]
        );
    }

    #[test]
    fn rejects_a_header_that_disagrees_with_the_records() {
        let text = MANIFEST.replace("# classes: 2", "# classes: 3");
        let error = Surface::parse(&text).expect_err("stale header");
        assert!(error.to_string().contains("declares 3 classes"), "{error}");
    }

    #[test]
    fn rejects_an_unknown_record_kind() {
        let error = Surface::parse("klass java/lang/Object\n").expect_err("bad kind");
        assert!(error.to_string().contains("unknown record kind"), "{error}");
    }

    #[test]
    fn rejects_a_record_with_an_extra_field() {
        let error = Surface::parse("class java/lang/Object extra\n").expect_err("extra field");
        assert!(error.to_string().contains("unexpected field"), "{error}");
    }
}
