//! A faithful writer for a parsed [`ClassFile`].
//!
//! [`ClassBuilder`](crate::ClassBuilder) names what it wants and lets the builder own the
//! constant-pool indices, which is right for fixtures but wrong for a class that has to
//! keep every raw attribute payload intact: `Signature`, annotations, `Record`,
//! `InnerClasses` and `Exceptions` all store pool indices inside their bytes. This writer
//! therefore re-emits the parsed pool at exactly the indices it was read from (including
//! the skipped half of a `long`/`double`) and copies every attribute payload verbatim.
//!
//! The result of `ClassFile::parse` followed by [`ClassFile::write`] is the original byte
//! sequence for every class file the parser accepts, unless the source used a non-canonical
//! modified-UTF-8 encoding or a lone surrogate, both of which the model decodes lossily.
//!
//! Writing is deliberately infallible: the input is an already-parsed model, so every
//! index and length already fit a class file.

use crate::constant_pool::{
    Constant, TAG_CLASS, TAG_DOUBLE, TAG_DYNAMIC, TAG_FIELDREF, TAG_FLOAT, TAG_INTEGER,
    TAG_INTERFACE_METHODREF, TAG_INVOKE_DYNAMIC, TAG_LONG, TAG_METHOD_HANDLE, TAG_METHOD_TYPE,
    TAG_METHODREF, TAG_MODULE, TAG_NAME_AND_TYPE, TAG_PACKAGE, TAG_STRING, TAG_UTF8,
};
use crate::model::{Attribute, ClassFile};
use crate::mutf8;

const MAGIC: u32 = 0xCAFE_BABE;

impl ClassFile {
    /// Serialises this class file, preserving pool indices and raw attribute payloads.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_be_bytes());
        out.extend_from_slice(&self.minor_version.to_be_bytes());
        out.extend_from_slice(&self.major_version.to_be_bytes());

        out.extend_from_slice(&(self.constant_pool.len() as u16).to_be_bytes());
        for (_, constant) in self.constant_pool.entries() {
            write_constant(&mut out, constant);
        }

        out.extend_from_slice(&self.access_flags.to_be_bytes());
        out.extend_from_slice(&self.this_class.to_be_bytes());
        out.extend_from_slice(&self.super_class.to_be_bytes());
        out.extend_from_slice(&(self.interfaces.len() as u16).to_be_bytes());
        for interface in &self.interfaces {
            out.extend_from_slice(&interface.to_be_bytes());
        }

        out.extend_from_slice(&(self.fields.len() as u16).to_be_bytes());
        for field in &self.fields {
            out.extend_from_slice(&field.access_flags.to_be_bytes());
            out.extend_from_slice(&field.name_index.to_be_bytes());
            out.extend_from_slice(&field.descriptor_index.to_be_bytes());
            write_attributes(&mut out, &field.attributes);
        }

        out.extend_from_slice(&(self.methods.len() as u16).to_be_bytes());
        for method in &self.methods {
            out.extend_from_slice(&method.access_flags.to_be_bytes());
            out.extend_from_slice(&method.name_index.to_be_bytes());
            out.extend_from_slice(&method.descriptor_index.to_be_bytes());
            write_attributes(&mut out, &method.attributes);
        }

        write_attributes(&mut out, &self.attributes);
        out
    }
}

fn write_attributes(out: &mut Vec<u8>, attributes: &[Attribute]) {
    out.extend_from_slice(&(attributes.len() as u16).to_be_bytes());
    for attribute in attributes {
        out.extend_from_slice(&attribute.name_index.to_be_bytes());
        out.extend_from_slice(&(attribute.data.len() as u32).to_be_bytes());
        out.extend_from_slice(&attribute.data);
    }
}

fn write_constant(out: &mut Vec<u8>, constant: &Constant) {
    match constant {
        Constant::Utf8(text) => {
            let bytes = mutf8::encode(text);
            out.push(TAG_UTF8);
            out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            out.extend_from_slice(&bytes);
        }
        Constant::Integer(value) => {
            out.push(TAG_INTEGER);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Constant::Float(value) => {
            out.push(TAG_FLOAT);
            out.extend_from_slice(&value.to_bits().to_be_bytes());
        }
        Constant::Long(value) => {
            out.push(TAG_LONG);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Constant::Double(value) => {
            out.push(TAG_DOUBLE);
            out.extend_from_slice(&value.to_bits().to_be_bytes());
        }
        Constant::Class { name_index } => {
            out.push(TAG_CLASS);
            out.extend_from_slice(&name_index.to_be_bytes());
        }
        Constant::String { string_index } => {
            out.push(TAG_STRING);
            out.extend_from_slice(&string_index.to_be_bytes());
        }
        Constant::FieldRef {
            class_index,
            name_and_type_index,
        } => {
            out.push(TAG_FIELDREF);
            out.extend_from_slice(&class_index.to_be_bytes());
            out.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        Constant::MethodRef {
            class_index,
            name_and_type_index,
        } => {
            out.push(TAG_METHODREF);
            out.extend_from_slice(&class_index.to_be_bytes());
            out.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        Constant::InterfaceMethodRef {
            class_index,
            name_and_type_index,
        } => {
            out.push(TAG_INTERFACE_METHODREF);
            out.extend_from_slice(&class_index.to_be_bytes());
            out.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        Constant::NameAndType {
            name_index,
            descriptor_index,
        } => {
            out.push(TAG_NAME_AND_TYPE);
            out.extend_from_slice(&name_index.to_be_bytes());
            out.extend_from_slice(&descriptor_index.to_be_bytes());
        }
        Constant::MethodHandle {
            reference_kind,
            reference_index,
        } => {
            out.push(TAG_METHOD_HANDLE);
            out.push(reference_kind.as_byte());
            out.extend_from_slice(&reference_index.to_be_bytes());
        }
        Constant::MethodType { descriptor_index } => {
            out.push(TAG_METHOD_TYPE);
            out.extend_from_slice(&descriptor_index.to_be_bytes());
        }
        Constant::Dynamic {
            bootstrap_method_attr_index,
            name_and_type_index,
        } => {
            out.push(TAG_DYNAMIC);
            out.extend_from_slice(&bootstrap_method_attr_index.to_be_bytes());
            out.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        Constant::InvokeDynamic {
            bootstrap_method_attr_index,
            name_and_type_index,
        } => {
            out.push(TAG_INVOKE_DYNAMIC);
            out.extend_from_slice(&bootstrap_method_attr_index.to_be_bytes());
            out.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        Constant::Module { name_index } => {
            out.push(TAG_MODULE);
            out.extend_from_slice(&name_index.to_be_bytes());
        }
        Constant::Package { name_index } => {
            out.push(TAG_PACKAGE);
            out.extend_from_slice(&name_index.to_be_bytes());
        }
    }
}
