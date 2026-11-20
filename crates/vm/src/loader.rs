//! Loading classes from a bootstrap path and an application path.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::ClassFile;

use crate::Shared;
use crate::class::{Class, ClassId, ClassState, FieldInfo, MethodInfo};
use crate::code;
use crate::error::VmError;
use crate::image::Image;
use crate::value::Value;

/// Where class files are read from.
///
/// Bootstrap first: ADR-0005 makes the replacement class library win over anything on the
/// application path, so `java/lang/System` resolves to our file and the JDK is never
/// opened at run time.
///
/// The bootstrap side has two sources: a directory (an extracted image, the debugging
/// fallback) and the JDK's `lib/modules`, which is what a real installation has (S4). A
/// directory hit wins, so an extracted class shadows the image without chipping at the file.
pub struct ClassPath {
    pub bootstrap: PathBuf,
    pub app: PathBuf,
    image: Option<Arc<Image>>,
}

impl ClassPath {
    pub fn new(bootstrap: impl Into<PathBuf>, app: impl Into<PathBuf>) -> Self {
        Self {
            bootstrap: bootstrap.into(),
            app: app.into(),
            image: None,
        }
    }

    /// Adds the module image as a bootstrap source, after the bootstrap directory.
    pub fn with_image(mut self, image: Arc<Image>) -> Self {
        self.image = Some(image);
        self
    }

    /// Opens `home/lib/modules` as the bootstrap source, keeping `home` as the directory
    /// fallback.
    pub fn from_jdk_home(
        home: impl Into<PathBuf>,
        app: impl Into<PathBuf>,
    ) -> Result<Self, crate::image::ImageError> {
        let home = home.into();
        let image = Image::open_jdk(&home)?;
        Ok(Self::new(home, app).with_image(Arc::new(image)))
    }

    /// The bytes of a class file, without loading or verifying it.
    ///
    /// Tooling needs to look at classes the VM has not loaded — the natives report walks
    /// constant pools — and going through [`Shared::load_class`] would run the verifier and
    /// decode every method for no reason.
    pub fn class_bytes(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError> {
        self.read(binary_name)
    }

    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError> {
        let relative = format!("{binary_name}.class");
        if let Ok(bytes) = std::fs::read(self.bootstrap.join(&relative)) {
            return Ok(Some(bytes));
        }
        if let Some(image) = &self.image
            && let Some(bytes) = image.class(binary_name)?
        {
            return Ok(Some(bytes.to_vec()));
        }
        if let Ok(bytes) = std::fs::read(self.app.join(&relative)) {
            return Ok(Some(bytes));
        }
        Ok(None)
    }
}

impl Shared {
    /// Loads a class by binary name, resolving its superclass on the way.
    pub fn load_class(&mut self, path: &ClassPath, name: &str) -> Result<ClassId, VmError> {
        if let Some(id) = self.class_ids.get(name) {
            return Ok(*id);
        }

        let class = build_class(self, path, name)?;
        if self.verify_on_load {
            for index in 0..class.methods.len() {
                let (method_name, descriptor, code) = {
                    let method = &class.methods[index];
                    (
                        method.name.clone(),
                        method.descriptor.clone(),
                        method.code.clone(),
                    )
                };
                let Some(code) = code else { continue };
                let qualified = format!("{}.{method_name}{descriptor}", class.name);
                crate::verify::verify(&code, &class, &qualified).map_err(|error| match error {
                    VmError::BadConstantPool(detail) => VmError::VerifyError { detail },
                    other => other,
                })?;
            }
        }
        let super_name = class.super_name.clone();
        let interface_names = class.interface_names.clone();
        let id = ClassId(self.classes.len() as u32);

        // Publish the id before resolving the superclass, so a malformed cycle terminates
        // instead of recursing forever.
        self.classes.push(class);
        self.class_ids.insert(name.to_owned(), id);

        if let Some(super_name) = super_name {
            let super_id = self.load_class(path, &super_name)?;
            self.classes[id.index()].super_class = Some(super_id);
        }
        // The layout needs the superclass layout, which only exists after linking.
        let mut layout = match self.classes[id.index()].super_class {
            Some(super_id) => self.classes[super_id.index()].layout.clone(),
            None => Vec::new(),
        };
        for field in &self.classes[id.index()].fields {
            if field.access_flags & crate::class::ACC_STATIC == 0 {
                layout.push(crate::class::FieldSlot {
                    name: field.name.clone(),
                    descriptor: field.descriptor.clone(),
                });
            }
        }
        self.classes[id.index()].layout = layout;

        // Interfaces are resolved after publication for the same reason as the superclass:
        // a malformed cycle terminates instead of recursing.
        for interface_name in interface_names {
            let interface_id = self.load_class(path, &interface_name)?;
            self.classes[id.index()].interfaces.push(interface_id);
        }
        Ok(id)
    }
}

/// Converts a `ConstantValue` entry to the value a static field of `descriptor` starts with.
///
/// JVMS §4.7.2 allows `Integer`, `Float`, `Long`, `Double` and `String` entries, and requires
/// the entry's type to match the field's descriptor: `byte`, `char`, `short` and `boolean`
/// fields take an `Integer`.
fn constant_value(
    shared: &mut Shared,
    path: &ClassPath,
    file: &ClassFile,
    constant: &rava_classfile::Constant,
    descriptor: &str,
) -> Result<Value, VmError> {
    use rava_classfile::Constant;
    let mismatch = || {
        VmError::BadConstantPool(format!(
            "field of type {descriptor} cannot take a {constant:?} ConstantValue"
        ))
    };
    match (descriptor, constant) {
        ("I" | "S" | "B" | "C" | "Z", Constant::Integer(value)) => Ok(Value::Int(*value)),
        ("J", Constant::Long(value)) => Ok(Value::Long(*value)),
        ("F", Constant::Float(value)) => Ok(Value::Float(*value)),
        ("D", Constant::Double(value)) => Ok(Value::Double(*value)),
        ("Ljava/lang/String;", Constant::String { string_index }) => {
            let text = file.constant_pool.utf8(*string_index)?.to_owned();
            Ok(Value::Ref(shared.intern(path, &text)?))
        }
        _ => Err(mismatch()),
    }
}

/// The primitive descriptor a wrapper class's `TYPE` field names, if it is one of them.
fn primitive_type_of(class_name: &str) -> Option<&'static str> {
    Some(match class_name {
        "java/lang/Integer" => "I",
        "java/lang/Long" => "J",
        "java/lang/Float" => "F",
        "java/lang/Double" => "D",
        "java/lang/Boolean" => "Z",
        "java/lang/Byte" => "B",
        "java/lang/Character" => "C",
        "java/lang/Short" => "S",
        "java/lang/Void" => "V",
        _ => return None,
    })
}

fn build_class(shared: &mut Shared, path: &ClassPath, name: &str) -> Result<Class, VmError> {
    let bytes = path
        .read(name)?
        .ok_or_else(|| VmError::ClassNotFound(name.to_owned()))?;
    let file = ClassFile::parse(&bytes)?;

    let mut methods = Vec::with_capacity(file.methods.len());
    for method in &file.methods {
        let method_name = file.method_name(method)?.to_owned();
        let descriptor = file.method_descriptor(method)?.to_owned();
        let qualified = format!("{name}.{method_name}{descriptor}");
        let decoded = match file.code(method)? {
            Some(attribute) => Some(Arc::new(code::decode(&attribute.data, &qualified)?)),
            None => None,
        };
        // Translate the line table's bytecode offsets into instruction indices, which is what
        // the frame's `pc` counts (ADR-0003: the interpreter never sees byte offsets).
        let mut lines: Vec<(u32, u16)> = Vec::new();
        if let Some(code) = &decoded {
            for (pc, line) in file.line_numbers(method)? {
                let index = match code.pcs.binary_search(&usize::from(pc)) {
                    Ok(found) => found,
                    Err(0) => 0,
                    Err(position) => position - 1,
                };
                lines.push((index as u32, line));
            }
            lines.sort_unstable();
            lines.dedup_by_key(|(index, _)| *index);
        }
        // A declared `native` binds from `natives`; an ordinary Java method binds from
        // `intrinsics`, which is how the VM replaces a host body that depends on VM-private
        // state (`Throwable.fillInStackTrace()`, see `Shared::intrinsics`).
        let native = if method.is_native() {
            shared
                .natives
                .get(&crate::native::key(name, &method_name, &descriptor))
                .copied()
        } else {
            shared
                .intrinsics
                .get(&crate::native::key(name, &method_name, &descriptor))
                .copied()
        };
        let qualified: Arc<str> = Arc::from(format!("{name}.{method_name}{descriptor}"));
        methods.push(MethodInfo {
            name: method_name,
            descriptor,
            access_flags: method.access_flags,
            is_native: method.is_native(),
            native,
            code: decoded,
            lines,
            qualified,
        });
    }

    let method_count = methods.len();
    let mut fields = Vec::with_capacity(file.fields.len());
    let mut statics = HashMap::new();
    for field in &file.fields {
        let field_name = file.field_name(field)?.to_owned();
        let descriptor = file.field_descriptor(field)?.to_owned();
        if field.access_flags & crate::class::ACC_STATIC != 0 {
            // JVMS §2.4: a static field starts at its type's default value; §5.4.2 preparation
            // then replaces it with the field's `ConstantValue` when the class file carries one.
            let value = match file.constant_value(field)? {
                Some(constant) => constant_value(shared, path, &file, constant, &descriptor)?,
                None => Value::default_for(&descriptor),
            };
            statics.insert(field_name.clone(), value);
        }
        fields.push(FieldInfo {
            name: field_name,
            descriptor,
            access_flags: field.access_flags,
        });
    }

    let super_name = file.super_class_name()?.map(str::to_owned);
    let interface_names = file
        .interface_names()?
        .into_iter()
        .map(str::to_owned)
        .collect();

    // `int.class` compiles to `getstatic java/lang/Integer.TYPE`, so the loader supplies that
    // value: a primitive class cannot be created in Java (issue #41).
    if let Some(descriptor) = primitive_type_of(name) {
        statics.insert(
            "TYPE".to_owned(),
            Value::Ref(shared.type_class(path, descriptor)?),
        );
    }

    let source_file = file.source_file()?;
    Ok(Class {
        name: name.to_owned(),
        file: Some(file),
        source_file,
        super_name,
        super_class: None,
        interface_names,
        interfaces: Vec::new(),
        methods,
        fields,
        invoke_cache: HashMap::new(),
        virtual_cache: HashMap::new(),
        invocations: vec![0; method_count],
        layout: Vec::new(),
        statics,
        indy_sites: HashMap::new(),
        condy: HashMap::new(),
        state: ClassState::Uninitialized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rava_classfile::{ClassBuilder, Constant};

    fn empty_class() -> ClassFile {
        let mut builder = ClassBuilder::new("Const", "java/lang/Object", 65);
        ClassFile::parse(&builder.build()).expect("fixture parses")
    }

    #[test]
    fn a_constant_of_the_wrong_type_is_rejected() {
        let file = empty_class();
        let mut shared = Shared::new();
        let path = ClassPath::new(".", ".");
        let error = constant_value(&mut shared, &path, &file, &Constant::Integer(1), "J")
            .expect_err("an int constant cannot initialise a long field");
        assert!(matches!(error, VmError::BadConstantPool(_)), "{error:?}");
    }
}
