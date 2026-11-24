//! A minimal class-file writer.
//!
//! Fixtures for dynamic constants, verifier cases and load tests would otherwise hand-
//! assemble class files byte by byte, and the same constant-pool index mistakes recur.
//! This module removes that failure mode: callers name what they want, the builder owns
//! the indices.
//!
//! What it is **not**, yet: an instruction assembler. A method body is given as raw bytes, so the
//! opcodes are still the caller's problem — labels, branches and the operand encodings are the next
//! increment. The names it knows are the ones the fixtures needed: `Utf8`, `Class`, `String`,
//! `Integer`, `Float`, `Long`, `Double`, `NameAndType`, `Fieldref`, `Methodref`,
//! `InterfaceMethodref`, `MethodType`, `MethodHandle`, `InvokeDynamic`, and one `Code`
//! attribute per method. A class-level
//! `BootstrapMethods` attribute is written when [`ClassBuilder::bootstrap_method`] (or
//! [`ClassBuilder::bootstrap_method_handle`]) has added an entry.

use std::collections::HashMap;

use crate::constant_pool::ReferenceKind;

/// One `BootstrapMethods` attribute entry under construction.
struct BootstrapEntry {
    method_handle: u16,
    arguments: Vec<u16>,
}

/// One field the caller wants in the class.
struct FieldEntry {
    access: u16,
    name: u16,
    descriptor: u16,
}

/// One method the caller wants in the class.
struct MethodEntry {
    access: u16,
    name: u16,
    descriptor: u16,
    code: Option<(u16, u16, Vec<u8>)>,
    /// `(start_pc, end_pc, handler_pc, catch_type)` — the exception table, by byte offset.
    handlers: Vec<(u16, u16, u16, u16)>,
}

/// A class file under construction.
pub struct ClassBuilder {
    entries: Vec<Vec<u8>>,
    index: HashMap<String, u16>,
    this_class: u16,
    super_class: u16,
    access: u16,
    major: u16,
    fields: Vec<FieldEntry>,
    interfaces: Vec<u16>,
    methods: Vec<MethodEntry>,
    /// The `BootstrapMethods` entries, in attribute order; an index into this vector is
    /// the `bootstrap_method_attr_index` of a `CONSTANT_InvokeDynamic`.
    bootstrap_methods: Vec<BootstrapEntry>,
    source_file: Option<u16>,
    /// The next constant-pool index; `long`/`double` consume two (JVMS §4.4.5).
    next_index: u16,
}

impl ClassBuilder {
    /// A class named `this` extending `super_name`, written as version `major`.
    pub fn new(this: &str, super_name: &str, major: u16) -> Self {
        let mut builder = Self::root(this, major);
        builder.super_class = builder.class(super_name);
        builder
    }

    /// A class named `this` with no superclass — the shape `java/lang/Object` needs.
    pub fn root(this: &str, major: u16) -> Self {
        let mut builder = Self {
            entries: Vec::new(),
            index: HashMap::new(),
            this_class: 0,
            super_class: 0,
            access: 0x0021, // public super
            major,
            fields: Vec::new(),
            interfaces: Vec::new(),
            methods: Vec::new(),
            bootstrap_methods: Vec::new(),
            source_file: None,
            next_index: 1,
        };
        builder.this_class = builder.class(this);
        builder
    }

    /// Replaces the class access flags (the default is `public super`, `0x0021`).
    pub fn with_access(mut self, access: u16) -> Self {
        self.access = access;
        self
    }

    /// Adds a `SourceFile` attribute, the class-level marker of where it came from.
    pub fn source_file(&mut self, name: &str) {
        self.source_file = Some(self.utf8(name));
    }

    /// A `CONSTANT_Utf8`, deduplicated.
    ///
    /// The payload is encoded as modified UTF-8 (CESU-8 for supplementary characters), the
    /// only encoding a class file accepts; plain UTF-8 would write a four-byte sequence.
    pub fn utf8(&mut self, text: &str) -> u16 {
        let key = format!("U{text}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let bytes = crate::mutf8::encode(text);
        let mut entry = vec![1u8];
        entry.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        entry.extend_from_slice(&bytes);
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_Class` naming `name`, deduplicated.
    pub fn class(&mut self, name: &str) -> u16 {
        let name_index = self.utf8(name);
        let key = format!("C{name}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![7u8];
        entry.extend_from_slice(&name_index.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_String`, deduplicated.
    pub fn string(&mut self, text: &str) -> u16 {
        let text_index = self.utf8(text);
        let key = format!("S{text}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![8u8];
        entry.extend_from_slice(&text_index.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_Long` (JVMS §4.4.5). It occupies two constant-pool entries, so the next index
    /// skips one.
    pub fn long(&mut self, value: i64) -> u16 {
        let key = format!("L{value}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![5u8];
        entry.extend_from_slice(&value.to_be_bytes());
        self.push(key, entry, 2)
    }

    /// A `CONSTANT_Integer` (JVMS §4.4.4), one entry wide.
    pub fn integer(&mut self, value: i32) -> u16 {
        let key = format!("I{value}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![3u8];
        entry.extend_from_slice(&value.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_Float` (JVMS §4.4.4), one entry wide.
    pub fn float(&mut self, value: f32) -> u16 {
        let key = format!("R{}", value.to_bits());
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![4u8];
        entry.extend_from_slice(&value.to_bits().to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_Double` (JVMS §4.4.5), also two entries wide.
    pub fn double(&mut self, value: f64) -> u16 {
        let key = format!("D{}", value.to_bits());
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![6u8];
        entry.extend_from_slice(&value.to_bits().to_be_bytes());
        self.push(key, entry, 2)
    }

    /// A `CONSTANT_NameAndType`, deduplicated.
    pub fn name_and_type(&mut self, name: &str, descriptor: &str) -> u16 {
        let name_index = self.utf8(name);
        let descriptor_index = self.utf8(descriptor);
        let key = format!("N{name}{descriptor}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![12u8];
        entry.extend_from_slice(&name_index.to_be_bytes());
        entry.extend_from_slice(&descriptor_index.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_Fieldref`, deduplicated.
    pub fn field_ref(&mut self, owner: &str, name: &str, descriptor: &str) -> u16 {
        let class_index = self.class(owner);
        let nat_index = self.name_and_type(name, descriptor);
        let key = format!("F{owner}.{name}{descriptor}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![9u8];
        entry.extend_from_slice(&class_index.to_be_bytes());
        entry.extend_from_slice(&nat_index.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_Methodref`, deduplicated.
    pub fn method_ref(&mut self, owner: &str, name: &str, descriptor: &str) -> u16 {
        let class_index = self.class(owner);
        let nat_index = self.name_and_type(name, descriptor);
        let key = format!("M{owner}.{name}{descriptor}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![10u8];
        entry.extend_from_slice(&class_index.to_be_bytes());
        entry.extend_from_slice(&nat_index.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_InterfaceMethodref`, deduplicated.
    ///
    /// `invokeinterface` names its target through this entry, not a `Methodref` (JVMS
    /// §4.4.2). The owner must be an interface.
    pub fn interface_method_ref(&mut self, owner: &str, name: &str, descriptor: &str) -> u16 {
        let class_index = self.class(owner);
        let nat_index = self.name_and_type(name, descriptor);
        let key = format!("X{owner}.{name}{descriptor}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![11u8];
        entry.extend_from_slice(&class_index.to_be_bytes());
        entry.extend_from_slice(&nat_index.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_MethodType` (JVMS §4.4.9), the method descriptor as a constant.
    ///
    /// `LambdaMetafactory` passes its SAM type and instantiated type as these entries.
    pub fn method_type(&mut self, descriptor: &str) -> u16 {
        let descriptor_index = self.utf8(descriptor);
        let key = format!("T{descriptor}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![16u8];
        entry.extend_from_slice(&descriptor_index.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_MethodHandle` (JVMS §4.4.8).
    pub fn method_handle(&mut self, reference_kind: ReferenceKind, reference_index: u16) -> u16 {
        let key = format!("H{}:{reference_index}", reference_kind.as_byte());
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![15u8, reference_kind.as_byte()];
        entry.extend_from_slice(&reference_index.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// A `CONSTANT_InvokeDynamic` naming bootstrap entry `bootstrap_index` and a
    /// call-site name/descriptor (JVMS §4.4.10).
    pub fn invoke_dynamic(&mut self, bootstrap_index: u16, name: &str, descriptor: &str) -> u16 {
        let name_and_type = self.name_and_type(name, descriptor);
        let key = format!("I{bootstrap_index}:{name}{descriptor}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![18u8];
        entry.extend_from_slice(&bootstrap_index.to_be_bytes());
        entry.extend_from_slice(&name_and_type.to_be_bytes());
        self.push(key, entry, 1)
    }

    /// Append a `BootstrapMethods` entry whose handle is `method_handle` and return its
    /// attribute index.
    ///
    /// `method_handle` must be a `CONSTANT_MethodHandle` index, typically from
    /// [`method_handle`](Self::method_handle).
    pub fn bootstrap_method_handle(&mut self, method_handle: u16, arguments: Vec<u16>) -> u16 {
        if let Some((index, _)) = self
            .bootstrap_methods
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.method_handle == method_handle && entry.arguments == arguments)
        {
            return index as u16;
        }
        let index = self.bootstrap_methods.len() as u16;
        self.bootstrap_methods.push(BootstrapEntry {
            method_handle,
            arguments,
        });
        index
    }

    /// Append a `BootstrapMethods` entry for a static bootstrap method and return its
    /// attribute index.
    ///
    /// This is the common shape: the handle is `REF_invokeStatic` to
    /// `owner.name:descriptor`, exactly as javac emits `StringConcatFactory` bootstraps.
    pub fn bootstrap_method(
        &mut self,
        owner: &str,
        name: &str,
        descriptor: &str,
        arguments: Vec<u16>,
    ) -> u16 {
        let reference = self.method_ref(owner, name, descriptor);
        let handle = self.method_handle(ReferenceKind::InvokeStatic, reference);
        self.bootstrap_method_handle(handle, arguments)
    }

    /// Declares that this class implements `name` (a binary name), in the order given.
    pub fn interface(&mut self, name: &str) {
        let index = self.class(name);
        if !self.interfaces.contains(&index) {
            self.interfaces.push(index);
        }
    }

    /// Adds a field. `access` is the raw access-flags word (`0x0009` is `public static`).
    pub fn field(&mut self, access: u16, name: &str, descriptor: &str) {
        let name = self.utf8(name);
        let descriptor = self.utf8(descriptor);
        self.fields.push(FieldEntry {
            access,
            name,
            descriptor,
        });
    }

    /// Adds a method. `code` is `(max_stack, max_locals, bytes)`; `None` means `native`/`abstract`.
    pub fn method(
        &mut self,
        access: u16,
        name: &str,
        descriptor: &str,
        code: Option<(u16, u16, Vec<u8>)>,
    ) {
        self.method_with_handlers(access, name, descriptor, code, Vec::new());
    }

    /// Like [`method`](Self::method), with an exception table. The entries name **byte offsets**,
    /// which the assembler does not expose yet, so a fixture that needs handlers has to count them.
    pub fn method_with_handlers(
        &mut self,
        access: u16,
        name: &str,
        descriptor: &str,
        code: Option<(u16, u16, Vec<u8>)>,
        handlers: Vec<(u16, u16, u16, u16)>,
    ) {
        let name = self.utf8(name);
        let descriptor = self.utf8(descriptor);
        self.methods.push(MethodEntry {
            access,
            name,
            descriptor,
            code,
            handlers,
        });
    }

    /// Serialises the class file.
    pub fn build(&mut self) -> Vec<u8> {
        let code_name = self.utf8("Code");
        let bootstrap_methods_name = if self.bootstrap_methods.is_empty() {
            None
        } else {
            Some(self.utf8("BootstrapMethods"))
        };
        let source_file_name = if self.source_file.is_some() {
            Some(self.utf8("SourceFile"))
        } else {
            None
        };

        let mut out = Vec::new();
        out.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
        out.extend_from_slice(&0u16.to_be_bytes()); // minor
        out.extend_from_slice(&self.major.to_be_bytes());
        out.extend_from_slice(&self.next_index.to_be_bytes());
        for entry in &self.entries {
            out.extend_from_slice(entry);
        }

        out.extend_from_slice(&self.access.to_be_bytes());
        out.extend_from_slice(&self.this_class.to_be_bytes());
        out.extend_from_slice(&self.super_class.to_be_bytes());
        out.extend_from_slice(&(self.interfaces.len() as u16).to_be_bytes());
        for interface in &self.interfaces {
            out.extend_from_slice(&interface.to_be_bytes());
        }
        out.extend_from_slice(&(self.fields.len() as u16).to_be_bytes());
        for field in &self.fields {
            out.extend_from_slice(&field.access.to_be_bytes());
            out.extend_from_slice(&field.name.to_be_bytes());
            out.extend_from_slice(&field.descriptor.to_be_bytes());
            out.extend_from_slice(&0u16.to_be_bytes()); // no field attributes
        }
        out.extend_from_slice(&(self.methods.len() as u16).to_be_bytes());

        for method in &self.methods {
            out.extend_from_slice(&method.access.to_be_bytes());
            out.extend_from_slice(&method.name.to_be_bytes());
            out.extend_from_slice(&method.descriptor.to_be_bytes());
            match &method.code {
                None => out.extend_from_slice(&0u16.to_be_bytes()),
                Some((max_stack, max_locals, code)) => {
                    out.extend_from_slice(&1u16.to_be_bytes()); // one attribute
                    let handlers = &method.handlers;
                    out.extend_from_slice(&code_name.to_be_bytes());
                    out.extend_from_slice(
                        &(12 + code.len() as u32 + 8 * handlers.len() as u32).to_be_bytes(),
                    );
                    out.extend_from_slice(&max_stack.to_be_bytes());
                    out.extend_from_slice(&max_locals.to_be_bytes());
                    out.extend_from_slice(&(code.len() as u32).to_be_bytes());
                    out.extend_from_slice(code);
                    out.extend_from_slice(&(handlers.len() as u16).to_be_bytes());
                    for (start, end, handler, catch_type) in handlers {
                        out.extend_from_slice(&start.to_be_bytes());
                        out.extend_from_slice(&end.to_be_bytes());
                        out.extend_from_slice(&handler.to_be_bytes());
                        out.extend_from_slice(&catch_type.to_be_bytes());
                    }
                    out.extend_from_slice(&0u16.to_be_bytes()); // nested attributes
                }
            }
        }

        let class_attribute_count =
            u16::from(self.source_file.is_some()) + u16::from(bootstrap_methods_name.is_some());
        out.extend_from_slice(&class_attribute_count.to_be_bytes());
        if let (Some(source), Some(name)) = (self.source_file, source_file_name) {
            out.extend_from_slice(&name.to_be_bytes());
            out.extend_from_slice(&2u32.to_be_bytes()); // attribute length
            out.extend_from_slice(&source.to_be_bytes());
        }
        if let Some(name) = bootstrap_methods_name {
            // count u2, then (method_handle u2, argument_count u2, arguments u2 each).
            let payload = 2
                + self.bootstrap_methods.len() as u32 * 4
                + self
                    .bootstrap_methods
                    .iter()
                    .map(|entry| entry.arguments.len() as u32 * 2)
                    .sum::<u32>();
            out.extend_from_slice(&name.to_be_bytes());
            out.extend_from_slice(&payload.to_be_bytes());
            out.extend_from_slice(&(self.bootstrap_methods.len() as u16).to_be_bytes());
            for entry in &self.bootstrap_methods {
                out.extend_from_slice(&entry.method_handle.to_be_bytes());
                out.extend_from_slice(&(entry.arguments.len() as u16).to_be_bytes());
                for argument in &entry.arguments {
                    out.extend_from_slice(&argument.to_be_bytes());
                }
            }
        }
        out
    }

    fn push(&mut self, key: String, entry: Vec<u8>, width: u16) -> u16 {
        let index = self.next_index;
        self.next_index += width;
        self.entries.push(entry);
        self.index.insert(key, index);
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClassFile;

    #[test]
    fn writes_access_flags_a_root_superclass_and_a_source_file() {
        let mut builder = ClassBuilder::root("java/lang/Object", 61).with_access(0x0421);
        builder.source_file("RavaStub.java");
        builder.method(0x0401, "toString", "()Ljava/lang/String;", None);

        let class = ClassFile::parse(&builder.build()).expect("builder output parses");
        assert_eq!(class.access_flags, 0x0421);
        assert_eq!(class.super_class_name().unwrap(), None);
        assert_eq!(
            class.source_file().unwrap().as_deref(),
            Some("RavaStub.java")
        );
        assert!(
            class
                .find_method("toString", "()Ljava/lang/String;")
                .unwrap()
                .is_some()
        );
    }
}
