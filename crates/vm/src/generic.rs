//! Generic reflection (M2.22): JVMS signatures resolved to `java.lang.reflect.Type`.
//!
//! The class-file `Signature` strings are parsed by `rava-classfile`'s metadata module.
//! This module turns the parsed graph into reflective objects: a raw class type is the
//! existing `Class` object (`java.lang.Class`
//! implements `Type`), while a parameterized type, type variable, wildcard and generic
//! array type are [`crate::HeapObject::Type`] objects cached structurally by
//! [`crate::Shared::type_object`].
//!
//! Names are resolved through the declaring class's defining loader, so a fixture class and
//! a library class keep their own identities. A class with no `Signature` attribute
//! reports its raw superclass, interfaces and member types as `Class` objects, exactly as
//! the JDK does; a malformed signature is the typed [`crate::VmError::Signature`], never a
//! guessed raw type.
//!
//! The type name a `toString`/`getTypeName` reports is deterministic and JDK-shaped but
//! not the JDK's exact formatter: a parameterized type prints `raw<arg, arg>`, a variable
//! its name, a wildcard `?`, `? extends X` or `? super X`, and a generic array `X[]`.

use rava_classfile::metadata::{
    ClassTypeSignature, MethodSignature, TypeArgument, TypeParameter, TypeSignature,
};

use crate::VmError;
use crate::heap::{
    ClassRef, GenericArrayType, HeapError, HeapObject, ObjRef, ParameterizedType, PrimitiveKind,
    ReflectMember, TypeDeclaration, TypeObject, TypeVariableType, WildcardType,
};
use crate::loader::{ClassId, LoaderId};
use crate::reflection::{
    ClassObject, allocate_reference_array, class_object_of, parameter_descriptors,
    return_descriptor,
};
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const TYPE: &str = "java/lang/reflect/Type";
const TYPE_VARIABLE: &str = "java/lang/reflect/TypeVariable";

/// Register the M2.22 generic-reflection natives.
///
/// The methods are keyed by the class that declares them: the generic queries on `Class`,
/// `Field`, `Method`, `Constructor` and `Executable`, the accessors on the four
/// `java.lang.reflect` interfaces, and `Type.getTypeName` (the C1 stub's default method has
/// no `Code`).
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        "java/lang/Class",
        "getGenericSuperclass",
        "()Ljava/lang/reflect/Type;",
        class_get_generic_superclass,
    );
    shared.register_native(
        "java/lang/Class",
        "getGenericInterfaces",
        "()[Ljava/lang/reflect/Type;",
        class_get_generic_interfaces,
    );
    shared.register_native(
        "java/lang/Class",
        "getTypeParameters",
        "()[Ljava/lang/reflect/TypeVariable;",
        class_get_type_parameters,
    );
    shared.register_native(
        "java/lang/Class",
        "getTypeName",
        "()Ljava/lang/String;",
        class_get_type_name,
    );
    shared.register_native(
        "java/lang/reflect/Field",
        "getGenericType",
        "()Ljava/lang/reflect/Type;",
        field_get_generic_type,
    );
    shared.register_native(
        "java/lang/reflect/Method",
        "getGenericReturnType",
        "()Ljava/lang/reflect/Type;",
        method_get_generic_return_type,
    );
    for owner in [
        "java/lang/reflect/Executable",
        "java/lang/reflect/Method",
        "java/lang/reflect/Constructor",
    ] {
        shared.register_native(
            owner,
            "getGenericParameterTypes",
            "()[Ljava/lang/reflect/Type;",
            executable_get_generic_parameter_types,
        );
        shared.register_native(
            owner,
            "getTypeParameters",
            "()[Ljava/lang/reflect/TypeVariable;",
            executable_get_type_parameters,
        );
    }
    shared.register_native(
        "java/lang/reflect/Type",
        "getTypeName",
        "()Ljava/lang/String;",
        type_get_type_name,
    );
    shared.register_native(
        "java/lang/reflect/ParameterizedType",
        "getActualTypeArguments",
        "()[Ljava/lang/reflect/Type;",
        parameterized_get_actual_type_arguments,
    );
    shared.register_native(
        "java/lang/reflect/ParameterizedType",
        "getRawType",
        "()Ljava/lang/reflect/Type;",
        parameterized_get_raw_type,
    );
    shared.register_native(
        "java/lang/reflect/ParameterizedType",
        "getOwnerType",
        "()Ljava/lang/reflect/Type;",
        parameterized_get_owner_type,
    );
    shared.register_native(
        "java/lang/reflect/TypeVariable",
        "getName",
        "()Ljava/lang/String;",
        type_variable_get_name,
    );
    shared.register_native(
        "java/lang/reflect/TypeVariable",
        "getBounds",
        "()[Ljava/lang/reflect/Type;",
        type_variable_get_bounds,
    );
    shared.register_native(
        "java/lang/reflect/TypeVariable",
        "getGenericDeclaration",
        "()Ljava/lang/reflect/GenericDeclaration;",
        type_variable_get_generic_declaration,
    );
    shared.register_native(
        "java/lang/reflect/WildcardType",
        "getUpperBounds",
        "()[Ljava/lang/reflect/Type;",
        wildcard_get_upper_bounds,
    );
    shared.register_native(
        "java/lang/reflect/WildcardType",
        "getLowerBounds",
        "()[Ljava/lang/reflect/Type;",
        wildcard_get_lower_bounds,
    );
    shared.register_native(
        "java/lang/reflect/GenericArrayType",
        "getGenericComponentType",
        "()Ljava/lang/reflect/Type;",
        generic_array_get_generic_component_type,
    );
}

/// The resolution context of one declaration.
///
/// `class_parameters` comes from the declaring class's `Signature`; `member_parameters`
/// from a method's or constructor's. A type-variable occurrence is looked up in the member
/// list first, because `<T> T method(T)` shadows the class's `T`.
struct Context<'a> {
    loader: LoaderId,
    class: ClassId,
    class_parameters: &'a [TypeParameter],
    member_parameters: &'a [TypeParameter],
    member_declaration: Option<&'a TypeDeclaration>,
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// Parse the class signature of a loaded class, or an empty parameter list when it is not
/// generic.
fn class_parameters(shared: &Shared, class: ClassId) -> Result<Vec<TypeParameter>, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let metadata = entry.metadata()?;
    Ok(metadata
        .class_signature()?
        .map(|signature| signature.type_parameters)
        .unwrap_or_default())
}

/// The generic view of a reflected field, method or constructor.
struct MemberGenerics {
    loader: LoaderId,
    class_parameters: Vec<TypeParameter>,
    member_parameters: Vec<TypeParameter>,
    declaration: TypeDeclaration,
    signature: Option<MethodSignature>,
}

impl MemberGenerics {
    fn context(&self) -> Context<'_> {
        Context {
            loader: self.loader,
            class: match self.declaration {
                TypeDeclaration::Class(class) => class,
                TypeDeclaration::Member { class, .. } => class,
            },
            class_parameters: &self.class_parameters,
            member_parameters: &self.member_parameters,
            member_declaration: Some(&self.declaration),
        }
    }
}

/// Read the generics of a reflected method or constructor.
fn member_generics(shared: &Shared, member: &ReflectMember) -> Result<MemberGenerics, VmError> {
    let entry = shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?;
    let metadata = entry.metadata()?;
    let class_parameters = metadata
        .class_signature()?
        .map(|signature| signature.type_parameters)
        .unwrap_or_default();
    let method = metadata
        .method(&member.name, &member.descriptor)
        .ok_or_else(|| {
            VmError::Native(format!(
                "class {} has no {} {}:{}",
                entry.name,
                member.kind.binary_name(),
                member.name,
                member.descriptor
            ))
        })?;
    let signature = method.method_signature()?;
    let member_parameters = signature
        .as_ref()
        .map(|signature| signature.type_parameters.clone())
        .unwrap_or_default();
    Ok(MemberGenerics {
        loader: entry.loader,
        class_parameters,
        member_parameters,
        declaration: TypeDeclaration::Member {
            kind: member.kind,
            class: member.class,
            name: member.name.clone(),
            descriptor: member.descriptor.clone(),
        },
        signature,
    })
}

/// The `java.lang.Object` class object, the bound of an unbounded type variable and the
/// upper bound of an unbounded wildcard.
fn object_class(shared: &Shared) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class("java/lang/Object")?;
    shared.class_object(class)
}

fn primitive_kind(primitive: rava_classfile::PrimitiveSignature) -> PrimitiveKind {
    use rava_classfile::PrimitiveSignature;
    match primitive {
        PrimitiveSignature::Boolean => PrimitiveKind::Boolean,
        PrimitiveSignature::Byte => PrimitiveKind::Byte,
        PrimitiveSignature::Char => PrimitiveKind::Char,
        PrimitiveSignature::Short => PrimitiveKind::Short,
        PrimitiveSignature::Int => PrimitiveKind::Int,
        PrimitiveSignature::Long => PrimitiveKind::Long,
        PrimitiveSignature::Float => PrimitiveKind::Float,
        PrimitiveSignature::Double => PrimitiveKind::Double,
    }
}

/// Resolve one parsed type to a reflective `Type` handle.
fn resolve_type_signature(
    shared: &Shared,
    context: &Context<'_>,
    signature: &TypeSignature,
) -> Result<ObjRef, VmError> {
    match signature {
        TypeSignature::Primitive(primitive) => {
            Ok(shared.primitive_class(primitive_kind(*primitive)))
        }
        TypeSignature::Class(class_type) => resolve_class_type(shared, context, class_type),
        TypeSignature::TypeVariable(name) => resolve_type_variable(shared, context, name),
        TypeSignature::Array(component) => match raw_class_ref(shared, context, signature)? {
            Some(class_ref) => shared.class_object_for_ref(&class_ref),
            None => {
                let component = resolve_type_signature(shared, context, component)?;
                Ok(shared.type_object(TypeObject::GenericArray(GenericArrayType { component })))
            }
        },
    }
}

/// The raw class identity a signature denotes, when it has no type arguments.
///
/// A raw type is a `Class` object in Java (`Class` implements `Type`); only a signature
/// with type arguments, a type variable or an array of one needs a wrapper object.
fn raw_class_ref(
    shared: &Shared,
    context: &Context<'_>,
    signature: &TypeSignature,
) -> Result<Option<ClassRef>, VmError> {
    match signature {
        TypeSignature::Primitive(primitive) => {
            Ok(Some(ClassRef::Primitive(primitive_kind(*primitive))))
        }
        TypeSignature::Class(class_type) => {
            if !class_type.arguments.is_empty() || class_type.owner.is_some() {
                return Ok(None);
            }
            let class = shared.load_class(context.loader, &class_type.name)?;
            Ok(Some(ClassRef::Named(class)))
        }
        TypeSignature::Array(component) => {
            Ok(raw_class_ref(shared, context, component)?.map(ClassRef::array))
        }
        TypeSignature::TypeVariable(_) => Ok(None),
    }
}

fn resolve_class_type(
    shared: &Shared,
    context: &Context<'_>,
    class_type: &ClassTypeSignature,
) -> Result<ObjRef, VmError> {
    if class_type.arguments.is_empty() && class_type.owner.is_none() {
        let class = match shared.load_class(context.loader, &class_type.name) {
            Ok(class) => class,
            Err(VmError::ClassNotFound { .. }) => {
                let throwable = crate::exceptions::new_throwable(
                    shared,
                    "java/lang/NoClassDefFoundError",
                    class_type.name.clone(),
                )?;
                return Err(VmError::Thrown(throwable));
            }
            Err(error) => return Err(error),
        };
        return shared.class_object(class);
    }
    let class = match shared.load_class(context.loader, &class_type.name) {
        Ok(class) => class,
        Err(VmError::ClassNotFound { .. }) => {
            let throwable = crate::exceptions::new_throwable(
                shared,
                "java/lang/NoClassDefFoundError",
                class_type.name.clone(),
            )?;
            return Err(VmError::Thrown(throwable));
        }
        Err(error) => return Err(error),
    };
    let owner = match &class_type.owner {
        Some(owner) => Some(resolve_class_type(shared, context, owner)?),
        // A signature that spells a member class without a `.Outer` suffix (`Map$Entry`)
        // still has an owner; the JDK derives it from the class's declaring class.
        None => match declaring_class(shared, class)? {
            Some(outer) => Some(shared.class_object(outer)?),
            None => None,
        },
    };
    let mut arguments = Vec::with_capacity(class_type.arguments.len());
    for argument in &class_type.arguments {
        arguments.push(resolve_type_argument(shared, context, argument)?);
    }
    Ok(
        shared.type_object(TypeObject::Parameterized(ParameterizedType {
            raw: ClassRef::Named(class),
            owner,
            arguments,
        })),
    )
}

/// The class a member class is declared in, from its `InnerClasses` entry.
fn declaring_class(shared: &Shared, class: ClassId) -> Result<Option<ClassId>, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let metadata = entry.metadata()?;
    let Some(inner) = metadata
        .inner_classes
        .iter()
        .find(|inner| inner.inner_class == metadata.binary_name)
    else {
        return Ok(None);
    };
    let Some(outer_name) = &inner.outer_class else {
        return Ok(None);
    };
    match shared.load_class(entry.loader, outer_name) {
        Ok(outer) => Ok(Some(outer)),
        // The outer class is not required to be loadable for the inner class to exist; an
        // absent one means the JDK's `getDeclaringClass` would also be null.
        Err(VmError::ClassNotFound { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

fn resolve_type_argument(
    shared: &Shared,
    context: &Context<'_>,
    argument: &TypeArgument,
) -> Result<ObjRef, VmError> {
    match argument {
        TypeArgument::Type(signature) => resolve_type_signature(shared, context, signature),
        TypeArgument::Unbounded => wildcard_object(shared, vec![object_class(shared)?], Vec::new()),
        TypeArgument::Extends(signature) => {
            let upper = resolve_type_signature(shared, context, signature)?;
            wildcard_object(shared, vec![upper], Vec::new())
        }
        TypeArgument::Super(signature) => {
            let lower = resolve_type_signature(shared, context, signature)?;
            let upper = object_class(shared)?;
            // A lower-bounded wildcard's upper bound is `Object` and its lower bound the
            // named type, exactly as the JDK reports `? super Integer`.
            wildcard_object(shared, vec![upper], vec![lower])
        }
    }
}

fn wildcard_object(
    shared: &Shared,
    upper: Vec<ObjRef>,
    lower: Vec<ObjRef>,
) -> Result<ObjRef, VmError> {
    Ok(shared.type_object(TypeObject::Wildcard(WildcardType { upper, lower })))
}

/// Resolve a type-variable occurrence through the member then the class parameters.
fn resolve_type_variable(
    shared: &Shared,
    context: &Context<'_>,
    name: &str,
) -> Result<ObjRef, VmError> {
    if let Some(parameter) = context
        .member_parameters
        .iter()
        .find(|parameter| parameter.name == name)
    {
        let declaration = context.member_declaration.cloned();
        return type_variable_object(shared, context, parameter, declaration);
    }
    if let Some(parameter) = context
        .class_parameters
        .iter()
        .find(|parameter| parameter.name == name)
    {
        return type_variable_object(
            shared,
            context,
            parameter,
            Some(TypeDeclaration::Class(context.class)),
        );
    }
    // A name outside the declaration's own parameters (for example one inherited from an
    // enclosing class whose signature this slice does not walk) keeps the name and reports
    // `Object` as its only bound, with no known declaration.
    let bounds = vec![object_class(shared)?];
    Ok(
        shared.type_object(TypeObject::TypeVariable(TypeVariableType {
            name: name.to_owned(),
            bounds,
            declaration: None,
        })),
    )
}

fn type_variable_object(
    shared: &Shared,
    context: &Context<'_>,
    parameter: &TypeParameter,
    declaration: Option<TypeDeclaration>,
) -> Result<ObjRef, VmError> {
    let mut bounds = Vec::new();
    for bound in parameter.bounds() {
        bounds.push(resolve_type_signature(shared, context, bound)?);
    }
    if bounds.is_empty() {
        bounds.push(object_class(shared)?);
    }
    Ok(
        shared.type_object(TypeObject::TypeVariable(TypeVariableType {
            name: parameter.name.clone(),
            bounds,
            declaration,
        })),
    )
}

/// A `Type[]` holding the given handles.
fn type_array(shared: &Shared, values: &[ObjRef]) -> Result<ObjRef, VmError> {
    let component = shared.load_library_class(TYPE)?;
    allocate_reference_array(shared, ClassRef::Named(component), values)
}

/// A `TypeVariable[]` holding the given handles.
fn type_variable_array(shared: &Shared, values: &[ObjRef]) -> Result<ObjRef, VmError> {
    let component = shared.load_library_class(TYPE_VARIABLE)?;
    allocate_reference_array(shared, ClassRef::Named(component), values)
}

/// The class object for a member type descriptor.
fn class_for_descriptor(
    shared: &Shared,
    loader: LoaderId,
    descriptor: &str,
) -> Result<ObjRef, VmError> {
    let class_ref = shared.class_ref_for_descriptor(loader, descriptor)?;
    shared.class_object_for_ref(&class_ref)
}

/// The reflected member behind a receiver.
fn member_of(shared: &Shared, receiver: ObjRef) -> Result<ReflectMember, VmError> {
    let heap = shared.heap();
    Ok(heap.reflect(receiver).map_err(heap_error)?.clone())
}

/// The `java.lang.reflect.Type` object behind a receiver.
fn type_receiver(shared: &Shared, receiver: ObjRef) -> Result<TypeObject, VmError> {
    let heap = shared.heap();
    match heap.get(receiver).map_err(heap_error)? {
        HeapObject::Type(ty) => Ok(ty.clone()),
        _ => Err(VmError::Native(format!(
            "{receiver} is not a java.lang.reflect.Type object"
        ))),
    }
}

fn wrong_type_kind(expected: &str) -> VmError {
    VmError::Native(format!("the receiver is not a {expected}"))
}

/// `Class.getGenericSuperclass()`.
///
/// A generic class resolves the `Signature`'s superclass type; a raw class reports its
/// superclass `Class` object; an interface, `java.lang.Object` and a primitive report null.
fn class_get_generic_superclass(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let ClassObject::Named(class) = class_object_of(shared, receiver)? else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let loader = entry.loader;
    let metadata = entry.metadata()?;
    // An interface reports no superclass even though its class file names Object as one,
    // and its `Signature`'s superclass is that same Object.
    if metadata.is_interface() {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    }
    if let Some(signature) = metadata.class_signature()? {
        let context = Context {
            loader,
            class,
            class_parameters: &signature.type_parameters,
            member_parameters: &[],
            member_declaration: None,
        };
        let superclass = resolve_class_type(shared, &context, &signature.superclass)?;
        return Ok(Some(Value::Ref(superclass)));
    }
    let Some(super_name) = metadata.super_class.clone() else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let super_class = match shared.load_class(loader, &super_name) {
        Ok(class) => class,
        Err(VmError::ClassNotFound { .. }) => {
            let throwable = crate::exceptions::new_throwable(
                shared,
                "java/lang/NoClassDefFoundError",
                super_name.clone(),
            )?;
            return Err(VmError::Thrown(throwable));
        }
        Err(error) => return Err(error),
    };
    let object = shared.class_object(super_class)?;
    Ok(Some(Value::Ref(object)))
}

/// `Class.getGenericInterfaces()`.
fn class_get_generic_interfaces(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let ClassObject::Named(class) = class_object_of(shared, receiver)? else {
        return Ok(Some(Value::Ref(type_array(shared, &[])?)));
    };
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let loader = entry.loader;
    let metadata = entry.metadata()?;
    let mut interfaces = Vec::new();
    if let Some(signature) = metadata.class_signature()? {
        let context = Context {
            loader,
            class,
            class_parameters: &signature.type_parameters,
            member_parameters: &[],
            member_declaration: None,
        };
        for interface in &signature.interfaces {
            interfaces.push(resolve_class_type(shared, &context, interface)?);
        }
    } else {
        for name in &metadata.interfaces {
            let interface = match shared.load_class(loader, name) {
                Ok(interface) => interface,
                Err(VmError::ClassNotFound { .. }) => {
                    let throwable = crate::exceptions::new_throwable(
                        shared,
                        "java/lang/NoClassDefFoundError",
                        name.clone(),
                    )?;
                    return Err(VmError::Thrown(throwable));
                }
                Err(error) => return Err(error),
            };
            interfaces.push(shared.class_object(interface)?);
        }
    }
    let array = type_array(shared, &interfaces)?;
    Ok(Some(Value::Ref(array)))
}

/// `Class.getTypeParameters()`: the class's own type variables, or an empty array.
fn class_get_type_parameters(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let ClassObject::Named(class) = class_object_of(shared, receiver)? else {
        return Ok(Some(Value::Ref(type_variable_array(shared, &[])?)));
    };
    let parameters = class_parameters(shared, class)?;
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let context = Context {
        loader: entry.loader,
        class,
        class_parameters: &parameters,
        member_parameters: &[],
        member_declaration: None,
    };
    let mut variables = Vec::with_capacity(parameters.len());
    for parameter in &parameters {
        variables.push(type_variable_object(
            shared,
            &context,
            parameter,
            Some(TypeDeclaration::Class(class)),
        )?);
    }
    let array = type_variable_array(shared, &variables)?;
    Ok(Some(Value::Ref(array)))
}

/// `Class.getTypeName()`: the class name, as `Class.getName()` reports it.
fn class_get_type_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = class_object_of(shared, receiver)?;
    let name = shared.class_ref_name(&class.class_ref())?;
    let name = shared.heap().allocate_string(name);
    Ok(Some(Value::Ref(name)))
}

/// `Field.getGenericType()`.
fn field_get_generic_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let entry = shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?;
    let loader = entry.loader;
    let metadata = entry.metadata()?;
    let field = metadata
        .field(&member.name)
        .filter(|field| field.descriptor == member.descriptor)
        .ok_or_else(|| {
            VmError::Native(format!(
                "class {} has no field {}:{}",
                entry.name, member.name, member.descriptor
            ))
        })?;
    let class_parameters = metadata
        .class_signature()?
        .map(|signature| signature.type_parameters)
        .unwrap_or_default();
    let result = match field.generic_type()? {
        Some(signature) => {
            let context = Context {
                loader,
                class: member.class,
                class_parameters: &class_parameters,
                member_parameters: &[],
                member_declaration: None,
            };
            resolve_type_signature(shared, &context, &signature)?
        }
        None => class_for_descriptor(shared, loader, &field.descriptor)?,
    };
    Ok(Some(Value::Ref(result)))
}

/// `Method.getGenericReturnType()`.
fn method_get_generic_return_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let generics = member_generics(shared, &member)?;
    let result = match generics.signature.as_ref() {
        Some(signature) => match &signature.return_type {
            Some(return_type) => resolve_type_signature(shared, &generics.context(), return_type)?,
            None => shared.primitive_class(PrimitiveKind::Void),
        },
        None => class_for_descriptor(
            shared,
            generics.loader,
            return_descriptor(&member.descriptor)?,
        )?,
    };
    Ok(Some(Value::Ref(result)))
}

/// `Method.getGenericParameterTypes`, `Constructor.getGenericParameterTypes` and the
/// `Executable` declaration they inherit.
fn executable_get_generic_parameter_types(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let generics = member_generics(shared, &member)?;
    let descriptors = parameter_descriptors(&member.descriptor)?;
    let mut parameters = Vec::with_capacity(descriptors.len());
    match generics.signature.as_ref() {
        Some(signature) => {
            if signature.parameters.len() != descriptors.len() {
                return Err(VmError::Native(format!(
                    "generic signature of {}:{} has {} parameter(s) but the descriptor has {}",
                    member.name,
                    member.descriptor,
                    signature.parameters.len(),
                    descriptors.len()
                )));
            }
            for parameter in &signature.parameters {
                parameters.push(resolve_type_signature(
                    shared,
                    &generics.context(),
                    parameter,
                )?);
            }
        }
        None => {
            for descriptor in descriptors {
                parameters.push(class_for_descriptor(shared, generics.loader, descriptor)?);
            }
        }
    }
    let array = type_array(shared, &parameters)?;
    Ok(Some(Value::Ref(array)))
}

/// `Method.getTypeParameters`, `Constructor.getTypeParameters` and the `Executable`
/// declaration.
fn executable_get_type_parameters(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let generics = member_generics(shared, &member)?;
    let mut variables = Vec::with_capacity(generics.member_parameters.len());
    for parameter in &generics.member_parameters {
        variables.push(type_variable_object(
            shared,
            &generics.context(),
            parameter,
            Some(generics.declaration.clone()),
        )?);
    }
    let array = type_variable_array(shared, &variables)?;
    Ok(Some(Value::Ref(array)))
}

/// `Type.getTypeName()`.
fn type_get_type_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let ty = type_receiver(shared, receiver)?;
    let text = render_type(shared, &ty)?;
    let text = shared.heap().allocate_string(text);
    Ok(Some(Value::Ref(text)))
}

fn parameterized_get_actual_type_arguments(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::Parameterized(parameterized) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.ParameterizedType"));
    };
    let array = type_array(shared, &parameterized.arguments)?;
    Ok(Some(Value::Ref(array)))
}

fn parameterized_get_raw_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::Parameterized(parameterized) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.ParameterizedType"));
    };
    let raw = shared.class_object_for_ref(&parameterized.raw)?;
    Ok(Some(Value::Ref(raw)))
}

fn parameterized_get_owner_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::Parameterized(parameterized) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.ParameterizedType"));
    };
    Ok(Some(Value::Ref(
        parameterized.owner.unwrap_or(ObjRef::NULL),
    )))
}

fn type_variable_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::TypeVariable(variable) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.TypeVariable"));
    };
    let name = shared.heap().allocate_string(variable.name);
    Ok(Some(Value::Ref(name)))
}

fn type_variable_get_bounds(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::TypeVariable(variable) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.TypeVariable"));
    };
    let array = type_array(shared, &variable.bounds)?;
    Ok(Some(Value::Ref(array)))
}

fn type_variable_get_generic_declaration(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::TypeVariable(variable) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.TypeVariable"));
    };
    let declaration = match variable.declaration {
        Some(TypeDeclaration::Class(class)) => Value::Ref(shared.class_object(class)?),
        Some(TypeDeclaration::Member {
            kind,
            class,
            name,
            descriptor,
        }) => Value::Ref(shared.reflect_member(class, kind, &name, &descriptor)?),
        None => Value::Ref(ObjRef::NULL),
    };
    Ok(Some(declaration))
}

fn wildcard_get_upper_bounds(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::Wildcard(wildcard) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.WildcardType"));
    };
    let array = type_array(shared, &wildcard.upper)?;
    Ok(Some(Value::Ref(array)))
}

fn wildcard_get_lower_bounds(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::Wildcard(wildcard) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.WildcardType"));
    };
    let array = type_array(shared, &wildcard.lower)?;
    Ok(Some(Value::Ref(array)))
}

fn generic_array_get_generic_component_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let TypeObject::GenericArray(array) = type_receiver(shared, receiver)? else {
        return Err(wrong_type_kind("java.lang.reflect.GenericArrayType"));
    };
    Ok(Some(Value::Ref(array.component)))
}

/// The textual form of a `Type` object, for `toString` and `Type.getTypeName`.
///
/// Returns `None` for a receiver that is not a [`crate::HeapObject::Type`], so the existing
/// `Object.toString` shape of a class object is unchanged.
pub fn type_text(shared: &Shared, receiver: ObjRef) -> Result<Option<String>, VmError> {
    let ty = {
        let heap = shared.heap();
        match heap.get(receiver).map_err(heap_error)? {
            HeapObject::Type(ty) => ty.clone(),
            _ => return Ok(None),
        }
    };
    Ok(Some(render_type(shared, &ty)?))
}

fn render_type(shared: &Shared, ty: &TypeObject) -> Result<String, VmError> {
    match ty {
        TypeObject::Parameterized(parameterized) => {
            let raw = shared.class_ref_name(&parameterized.raw)?;
            let mut arguments = Vec::with_capacity(parameterized.arguments.len());
            for argument in &parameterized.arguments {
                arguments.push(render_any_type(shared, *argument)?);
            }
            Ok(format!("{raw}<{}>", arguments.join(", ")))
        }
        TypeObject::TypeVariable(variable) => Ok(variable.name.clone()),
        TypeObject::Wildcard(wildcard) => {
            if let Some(lower) = wildcard.lower.first() {
                return Ok(format!("? super {}", render_any_type(shared, *lower)?));
            }
            match wildcard.upper.first() {
                Some(upper) if render_any_type(shared, *upper)? != "java.lang.Object" => {
                    Ok(format!("? extends {}", render_any_type(shared, *upper)?))
                }
                _ => Ok("?".to_owned()),
            }
        }
        TypeObject::GenericArray(array) => {
            Ok(format!("{}[]", render_any_type(shared, array.component)?))
        }
    }
}

fn render_any_type(shared: &Shared, reference: ObjRef) -> Result<String, VmError> {
    if let Some(text) = type_text(shared, reference)? {
        return Ok(text);
    }
    let class_ref = class_object_of(shared, reference)?.class_ref();
    shared.class_ref_name(&class_ref)
}
