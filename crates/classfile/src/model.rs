//! The parsed shape of a class file (JVMS §4.1).

use crate::constant_pool::ConstantPool;
use crate::error::Error;
use crate::reader::Reader;

pub const ACC_STATIC: u16 = 0x0008;
pub const ACC_NATIVE: u16 = 0x0100;
pub const ACC_ABSTRACT: u16 = 0x0400;

/// An attribute kept as raw bytes.
///
/// The parser never rejects an unknown attribute name: JVMS §4.7 requires unknown
/// attributes to be ignored, and keeping the payload is what makes a class file from a
/// newer JDK parse instead of fail (R2). Typed decoders — starting with `Code` — read
/// these bytes later, and an attribute this VM does not understand is simply skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name_index: u16,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub access_flags: u16,
    pub name_index: u16,
    pub descriptor_index: u16,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    pub access_flags: u16,
    pub name_index: u16,
    pub descriptor_index: u16,
    pub attributes: Vec<Attribute>,
}

impl Method {
    pub fn is_static(&self) -> bool {
        self.access_flags & ACC_STATIC != 0
    }

    pub fn is_native(&self) -> bool {
        self.access_flags & ACC_NATIVE != 0
    }

    pub fn is_abstract(&self) -> bool {
        self.access_flags & ACC_ABSTRACT != 0
    }

    /// `native` and `abstract` methods carry no `Code` attribute (JVMS §4.7.3), so a
    /// missing body is not an error.
    pub fn has_code(&self) -> bool {
        !self.is_native() && !self.is_abstract()
    }
}

/// One entry of the `BootstrapMethods` attribute (JVMS §4.7.23).
///
/// `invokedynamic` and `condy` name a bootstrap method by index into this table; the method
/// itself is a `CONSTANT_MethodHandle` plus a list of constant-pool arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapMethod {
    pub method_ref: u16,
    pub arguments: Vec<u16>,
}

/// One `requires` entry of a module descriptor (JVMS §4.7.25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRequires {
    pub name: String,
    pub flags: u16,
    pub version: Option<String>,
}

/// One `exports` or `opens` entry: a package, its flags and the modules it is limited to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleExport {
    pub package: String,
    pub flags: u16,
    /// Empty means "to every module".
    pub to: Vec<String>,
}

/// One `provides` entry: a service and the implementations offered for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleProvides {
    pub service: String,
    pub with: Vec<String>,
}

/// A `module-info.class` descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInfo {
    pub name: String,
    pub flags: u16,
    pub version: Option<String>,
    pub requires: Vec<ModuleRequires>,
    pub exports: Vec<ModuleExport>,
    pub opens: Vec<ModuleExport>,
    pub uses: Vec<String>,
    pub provides: Vec<ModuleProvides>,
}

#[derive(Debug, Clone)]
pub struct ClassFile {
    pub minor_version: u16,
    pub major_version: u16,
    pub constant_pool: ConstantPool,
    pub access_flags: u16,
    pub this_class: u16,
    pub super_class: u16,
    pub interfaces: Vec<u16>,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub attributes: Vec<Attribute>,
    pub bootstrap_methods: Vec<BootstrapMethod>,
}

impl ClassFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(bytes);

        let magic = r.u4()?;
        if magic != 0xCAFE_BABE {
            return Err(Error::BadMagic { found: magic });
        }
        let minor_version = r.u2()?;
        let major_version = r.u2()?;

        // The version is recorded, never enforced: policy belongs to the loader
        // (ADR-0001, R2, R10).
        let constant_pool = ConstantPool::parse(&mut r)?;
        let access_flags = r.u2()?;
        let this_class = r.u2()?;
        let super_class = r.u2()?;

        let interface_count = r.u2()?;
        let mut interfaces = Vec::with_capacity(usize::from(interface_count));
        for _ in 0..interface_count {
            interfaces.push(r.u2()?);
        }

        let fields = parse_fields(&mut r)?;
        let methods = parse_methods(&mut r)?;
        let attributes = parse_attributes(&mut r)?;
        let bootstrap_methods = parse_bootstrap_methods(&constant_pool, &attributes)?;

        Ok(Self {
            minor_version,
            major_version,
            constant_pool,
            access_flags,
            this_class,
            super_class,
            interfaces,
            fields,
            methods,
            attributes,
            bootstrap_methods,
        })
    }

    pub fn this_class_name(&self) -> Result<&str, Error> {
        self.constant_pool.class_name(self.this_class)
    }

    pub fn super_class_name(&self) -> Result<Option<&str>, Error> {
        if self.super_class == 0 {
            Ok(None)
        } else {
            Ok(Some(self.constant_pool.class_name(self.super_class)?))
        }
    }

    pub fn interface_names(&self) -> Result<Vec<&str>, Error> {
        self.interfaces
            .iter()
            .map(|index| self.constant_pool.class_name(*index))
            .collect()
    }

    pub fn method_name(&self, method: &Method) -> Result<&str, Error> {
        self.constant_pool.utf8(method.name_index)
    }

    pub fn method_descriptor(&self, method: &Method) -> Result<&str, Error> {
        self.constant_pool.utf8(method.descriptor_index)
    }

    pub fn field_name(&self, field: &Field) -> Result<&str, Error> {
        self.constant_pool.utf8(field.name_index)
    }

    pub fn field_descriptor(&self, field: &Field) -> Result<&str, Error> {
        self.constant_pool.utf8(field.descriptor_index)
    }

    pub fn attribute_name(&self, attribute: &Attribute) -> Result<&str, Error> {
        self.constant_pool.utf8(attribute.name_index)
    }

    pub fn find_method(&self, name: &str, descriptor: &str) -> Result<Option<&Method>, Error> {
        for method in &self.methods {
            if self.method_name(method)? == name && self.method_descriptor(method)? == descriptor {
                return Ok(Some(method));
            }
        }
        Ok(None)
    }

    /// Returns the raw `Code` attribute of a method, if it has one.
    /// The `Module` attribute, if this is a module descriptor (JVMS §4.7.25).
    pub fn module(&self) -> Result<Option<ModuleInfo>, Error> {
        for attribute in &self.attributes {
            if self.attribute_name(attribute)? != "Module" {
                continue;
            }
            return Ok(Some(parse_module(&self.constant_pool, &attribute.data)?));
        }
        Ok(None)
    }

    /// The `SourceFile` attribute's value, used by stack traces.
    pub fn source_file(&self) -> Result<Option<String>, Error> {
        for attribute in &self.attributes {
            if self.attribute_name(attribute)? == "SourceFile" {
                let mut reader = Reader::new(&attribute.data);
                let index = reader.u2()?;
                return Ok(Some(self.constant_pool.utf8(index)?.to_owned()));
            }
        }
        Ok(None)
    }

    /// The method's `LineNumberTable`, as `(start_pc, line)` pairs in table order.
    ///
    /// Nested inside the `Code` attribute, after the bytecode and the exception table; every
    /// other nested attribute is skipped by length.
    pub fn line_numbers(&self, method: &Method) -> Result<Vec<(u16, u16)>, Error> {
        let Some(code) = self.code(method)? else {
            return Ok(Vec::new());
        };
        let mut reader = Reader::new(&code.data);
        reader.u2()?; // max_stack
        reader.u2()?; // max_locals
        let code_length = reader.u4()?;
        reader.skip(code_length as usize)?;
        let exception_count = reader.u2()?;
        reader.skip(usize::from(exception_count) * 8)?;
        let attribute_count = reader.u2()?;

        let mut lines = Vec::new();
        for _ in 0..attribute_count {
            let name_index = reader.u2()?;
            let length = reader.u4()?;
            let data = reader.bytes(length as usize)?;
            let attribute = Attribute {
                name_index,
                data: data.to_vec(),
            };
            if self.attribute_name(&attribute)? != "LineNumberTable" {
                continue;
            }
            let mut table = Reader::new(data);
            let entries = table.u2()?;
            for _ in 0..entries {
                let start_pc = table.u2()?;
                let line = table.u2()?;
                lines.push((start_pc, line));
            }
        }
        Ok(lines)
    }

    pub fn code<'a>(&self, method: &'a Method) -> Result<Option<&'a Attribute>, Error> {
        for attribute in &method.attributes {
            if self.attribute_name(attribute)? == "Code" {
                return Ok(Some(attribute));
            }
        }
        Ok(None)
    }
}

/// Reads the `BootstrapMethods` attribute, if the class has one.
///
/// It is parsed eagerly because an `invokedynamic` call site cannot be resolved without it:
/// the attribute is the only place a call site's bootstrap method exists.
fn parse_bootstrap_methods(
    pool: &ConstantPool,
    attributes: &[Attribute],
) -> Result<Vec<BootstrapMethod>, Error> {
    for attribute in attributes {
        if pool.utf8(attribute.name_index)? != "BootstrapMethods" {
            continue;
        }
        let mut reader = Reader::new(&attribute.data);
        let count = reader.u2()?;
        let mut methods = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let method_ref = reader.u2()?;
            let argument_count = reader.u2()?;
            let mut arguments = Vec::with_capacity(usize::from(argument_count));
            for _ in 0..argument_count {
                arguments.push(reader.u2()?);
            }
            methods.push(BootstrapMethod {
                method_ref,
                arguments,
            });
        }
        return Ok(methods);
    }
    Ok(Vec::new())
}

/// Reads the `Module` attribute's payload (JVMS §4.7.25).
fn parse_module(pool: &ConstantPool, data: &[u8]) -> Result<ModuleInfo, Error> {
    let mut r = Reader::new(data);
    // The module's own name is a `CONSTANT_Module`, not a Utf8 (JVMS §4.7.25).
    let name = pool.module_name(r.u2()?)?.to_owned();
    let flags = r.u2()?;
    let version_index = r.u2()?;
    let version = if version_index == 0 {
        None
    } else {
        Some(pool.utf8(version_index)?.to_owned())
    };

    let requires_count = r.u2()?;
    let mut requires = Vec::with_capacity(usize::from(requires_count));
    for _ in 0..requires_count {
        let module_index = r.u2()?;
        let flags = r.u2()?;
        let version_index = r.u2()?;
        requires.push(ModuleRequires {
            name: pool.module_name(module_index)?.to_owned(),
            flags,
            version: if version_index == 0 {
                None
            } else {
                Some(pool.utf8(version_index)?.to_owned())
            },
        });
    }

    let exports_count = r.u2()?;
    let mut exports = Vec::with_capacity(usize::from(exports_count));
    for _ in 0..exports_count {
        let package_index = r.u2()?;
        let flags = r.u2()?;
        let to_count = r.u2()?;
        let mut to = Vec::with_capacity(usize::from(to_count));
        for _ in 0..to_count {
            to.push(pool.module_name(r.u2()?)?.to_owned());
        }
        exports.push(ModuleExport {
            package: pool.package_name(package_index)?.to_owned(),
            flags,
            to,
        });
    }

    let opens_count = r.u2()?;
    let mut opens = Vec::with_capacity(usize::from(opens_count));
    for _ in 0..opens_count {
        let package_index = r.u2()?;
        let flags = r.u2()?;
        let to_count = r.u2()?;
        let mut to = Vec::with_capacity(usize::from(to_count));
        for _ in 0..to_count {
            to.push(pool.module_name(r.u2()?)?.to_owned());
        }
        opens.push(ModuleExport {
            package: pool.package_name(package_index)?.to_owned(),
            flags,
            to,
        });
    }

    let uses_count = r.u2()?;
    let mut uses = Vec::with_capacity(usize::from(uses_count));
    for _ in 0..uses_count {
        uses.push(pool.class_name(r.u2()?)?.to_owned());
    }

    let provides_count = r.u2()?;
    let mut provides = Vec::with_capacity(usize::from(provides_count));
    for _ in 0..provides_count {
        let service = pool.class_name(r.u2()?)?.to_owned();
        let with_count = r.u2()?;
        let mut with = Vec::with_capacity(usize::from(with_count));
        for _ in 0..with_count {
            with.push(pool.class_name(r.u2()?)?.to_owned());
        }
        provides.push(ModuleProvides { service, with });
    }

    Ok(ModuleInfo {
        name,
        flags,
        version,
        requires,
        exports,
        opens,
        uses,
        provides,
    })
}

fn parse_attributes(r: &mut Reader<'_>) -> Result<Vec<Attribute>, Error> {
    let count = r.u2()?;
    let mut attributes = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let name_index = r.u2()?;
        // u32 to usize has no `From` impl; a class file cannot exceed 4 GiB by format,
        // so the widening is lossless on every supported target.
        let length = r.u4()? as usize;
        let data = r.bytes(length)?.to_vec();
        attributes.push(Attribute { name_index, data });
    }
    Ok(attributes)
}

fn parse_fields(r: &mut Reader<'_>) -> Result<Vec<Field>, Error> {
    let count = r.u2()?;
    let mut fields = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        fields.push(Field {
            access_flags: r.u2()?,
            name_index: r.u2()?,
            descriptor_index: r.u2()?,
            attributes: parse_attributes(r)?,
        });
    }
    Ok(fields)
}

fn parse_methods(r: &mut Reader<'_>) -> Result<Vec<Method>, Error> {
    let count = r.u2()?;
    let mut methods = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        methods.push(Method {
            access_flags: r.u2()?,
            name_index: r.u2()?,
            descriptor_index: r.u2()?,
            attributes: parse_attributes(r)?,
        });
    }
    Ok(methods)
}
