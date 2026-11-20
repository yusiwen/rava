//! A minimal class-file writer (G5's first increment).
//!
//! Three slices (S20's condy fixture, S31's verifier fixtures, S32's load test) hand-assembled
//! class files byte by byte, and two of them got a constant-pool index wrong in the same way. This
//! module removes that failure mode: callers name what they want, the builder owns the indices.
//!
//! What it is **not**, yet: an instruction assembler. A method body is given as raw bytes, so the
//! opcodes are still the caller's problem — labels, branches and the operand encodings are the next
//! increment. The names it knows are the ones the fixtures needed: `Utf8`, `Class`, `String`,
//! `NameAndType`, `Fieldref`, `Methodref`, and one `Code` attribute per method.

use std::collections::HashMap;

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
    major: u16,
    fields: Vec<FieldEntry>,
    methods: Vec<MethodEntry>,
    /// The next constant-pool index; `long`/`double` consume two (JVMS §4.4.5).
    next_index: u16,
}

impl ClassBuilder {
    /// A class named `this` extending `super_name`, written as version `major`.
    pub fn new(this: &str, super_name: &str, major: u16) -> Self {
        let mut builder = Self {
            entries: Vec::new(),
            index: HashMap::new(),
            this_class: 0,
            super_class: 0,
            major,
            fields: Vec::new(),
            methods: Vec::new(),
            next_index: 1,
        };
        builder.this_class = builder.class(this);
        builder.super_class = builder.class(super_name);
        builder
    }

    /// A `CONSTANT_Utf8`, deduplicated.
    pub fn utf8(&mut self, text: &str) -> u16 {
        let key = format!("U{text}");
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let mut entry = vec![1u8];
        entry.extend_from_slice(&(text.len() as u16).to_be_bytes());
        entry.extend_from_slice(text.as_bytes());
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

        let mut out = Vec::new();
        out.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
        out.extend_from_slice(&0u16.to_be_bytes()); // minor
        out.extend_from_slice(&self.major.to_be_bytes());
        out.extend_from_slice(&self.next_index.to_be_bytes());
        for entry in &self.entries {
            out.extend_from_slice(entry);
        }

        out.extend_from_slice(&0x0021u16.to_be_bytes()); // public super
        out.extend_from_slice(&self.this_class.to_be_bytes());
        out.extend_from_slice(&self.super_class.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // interfaces
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

        out.extend_from_slice(&0u16.to_be_bytes()); // class attributes
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
