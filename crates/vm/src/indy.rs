//! `invokedynamic` call sites and the bootstrap methods they name (JVMS §5.4.3.6).
//!
//! A call site is resolved once and cached on its class. Resolution reads the
//! `BootstrapMethods` attribute, walks the method handle to an `(owner, name)` pair, and then
//! decides what the call site does. The bootstraps the corpus needs are implemented —
//! `StringConcatFactory`, `LambdaMetafactory`, `ObjectMethods` and `SwitchBootstraps`; anything
//! else is reported by name, which keeps a future JDK's new bootstrap a diagnosis instead of a
//! misparse.
//!
//! Running the real bootstrap method would mean implementing `java.lang.invoke` — a library
//! larger than this VM — so recognition replaces execution (ADR-0010).

use rava_classfile::{BootstrapMethod, Constant, ConstantPool};

use crate::Shared;
use crate::class::ClassId;
use crate::error::VmError;
use crate::loader::ClassPath;

/// One piece of a `makeConcatWithConstants` recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipePart {
    Text(String),
    /// `\u0001`: the next dynamic argument, rendered as text.
    Argument,
    /// `\u0002`: the next bootstrap-method argument, which must be a string constant.
    Constant(String),
}

/// Which generated method of a record a call site produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordMethodKind {
    ToString,
    Equals,
    HashCode,
}

/// One record component, as `ObjectMethods` receives it: a name and a field handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub descriptor: String,
    /// Slot in the record class's field layout.
    pub slot: usize,
}

/// What a call site does once resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallSite {
    /// `StringConcatFactory.makeConcatWithConstants` / `makeConcat`.
    Concat { recipe: Vec<RecipePart> },
    /// `LambdaMetafactory.metafactory`: calling the functional interface method on the
    /// produced object invokes `target_method` on `target_class`.
    Lambda {
        target_class: ClassId,
        target_method: usize,
        /// The interface method this lambda implements; only calls to it run the target.
        sam_name: String,
        sam_descriptor: String,
    },
    /// `ObjectMethods.bootstrap`: a record's `toString`, `equals` or `hashCode`.
    RecordMethod {
        kind: RecordMethodKind,
        class_name: String,
        components: Vec<Component>,
    },
    /// `SwitchBootstraps.typeSwitch`: a pattern switch. Calling the site with `(value, start)`
    /// returns the index of the first case class the value is an instance of, `cases.len()`
    /// when none matches, and `-1` for `null` — which is how javac's switch reaches a
    /// `case null` arm.
    TypeSwitch { cases: Vec<ClassId> },
    /// A bootstrap method this VM does not implement yet.
    Unsupported { bootstrap: String },
}

/// A resolved call site together with the descriptor it was called with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSiteInfo {
    pub descriptor: String,
    pub site: CallSite,
}

/// What phase one of resolution found, before any class is loaded.
enum Pending {
    Ready(CallSite),
    /// `ObjectMethods.bootstrap`: the record class and its component handles still have to be
    /// turned into field slots.
    RecordMethod {
        kind: RecordMethodKind,
        class_name: String,
        component_names: Vec<String>,
        handles: Vec<u16>,
    },
    /// A `LambdaMetafactory` call site whose implementation method still has to be resolved.
    Lambda {
        owner: String,
        name: String,
        descriptor: String,
        sam_name: String,
        sam_descriptor: String,
    },
    /// A `SwitchBootstraps.typeSwitch` call site whose case classes still have to be loaded.
    TypeSwitch {
        names: Vec<String>,
    },
    Unsupported(String),
}

pub fn resolve(
    shared: &mut Shared,
    path: &ClassPath,
    class: ClassId,
    index: u16,
) -> Result<CallSiteInfo, VmError> {
    // Phase one only reads the class file, so the borrow ends before any class is loaded.
    let (descriptor, pending) = {
        let class_ref = shared.class(class);
        let file = class_ref.file.as_ref().ok_or_else(|| {
            VmError::BadConstantPool(format!(
                "{} is synthetic and has no constants",
                class_ref.name
            ))
        })?;
        let pool = &file.constant_pool;

        let (bootstrap_index, name_and_type) = match pool.get(index)? {
            Constant::InvokeDynamic {
                bootstrap_method_attr_index,
                name_and_type_index,
            } => (*bootstrap_method_attr_index, *name_and_type_index),
            other => {
                return Err(VmError::BadConstantPool(format!(
                    "entry {index} is {other:?}, expected InvokeDynamic"
                )));
            }
        };
        let (name, descriptor) = pool.name_and_type(name_and_type)?;
        let descriptor = descriptor.to_owned();

        let bootstrap = file
            .bootstrap_methods
            .get(usize::from(bootstrap_index))
            .ok_or_else(|| {
                VmError::BadConstantPool(format!(
                    "invokedynamic #{index} ({name}{descriptor}) names bootstrap method \
                     {bootstrap_index}, which is absent"
                ))
            })?;

        let (owner, method_name) = bootstrap_target(pool, bootstrap)?;
        let pending = match (owner.as_str(), method_name.as_str()) {
            ("java/lang/invoke/StringConcatFactory", "makeConcatWithConstants") => {
                Pending::Ready(CallSite::Concat {
                    recipe: recipe_with_constants(pool, bootstrap)?,
                })
            }
            ("java/lang/invoke/StringConcatFactory", "makeConcat") => {
                Pending::Ready(CallSite::Concat {
                    // `makeConcat` has no recipe: every parameter is concatenated in order.
                    recipe: vec![
                        RecipePart::Argument;
                        crate::resolve::parameter_count(&descriptor)
                    ],
                })
            }
            ("java/lang/invoke/LambdaMetafactory", "metafactory")
            | ("java/lang/invoke/LambdaMetafactory", "altMetafactory") => {
                // Bootstrap arguments: samMethodType, implMethod, instantiatedMethodType.
                let handle_index = *bootstrap.arguments.get(1).ok_or_else(|| {
                    VmError::BadConstantPool(
                        "LambdaMetafactory has no implementation handle argument".to_owned(),
                    )
                })?;
                let (impl_owner, impl_name, impl_descriptor) = handle_target(pool, handle_index)?;
                // Bootstrap argument 0 is `samMethodType`, the interface method's descriptor.
                let sam_type_index = *bootstrap.arguments.first().ok_or_else(|| {
                    VmError::BadConstantPool(
                        "LambdaMetafactory has no samMethodType argument".to_owned(),
                    )
                })?;
                let sam_descriptor = match pool.get(sam_type_index)? {
                    Constant::MethodType { descriptor_index } => {
                        pool.utf8(*descriptor_index)?.to_owned()
                    }
                    other => {
                        return Err(VmError::BadConstantPool(format!(
                            "LambdaMetafactory's first argument is {other:?}, expected a MethodType"
                        )));
                    }
                };
                Pending::Lambda {
                    owner: impl_owner,
                    name: impl_name,
                    descriptor: impl_descriptor,
                    sam_name: name.to_owned(),
                    sam_descriptor,
                }
            }
            ("java/lang/runtime/SwitchBootstraps", "typeSwitch") => {
                // One `Class` constant per case, in the order javac numbered them; the index
                // this produces is what the following `tableswitch`/`lookupswitch` reads.
                let mut names = Vec::with_capacity(bootstrap.arguments.len());
                for argument in &bootstrap.arguments {
                    match pool.get(*argument)? {
                        Constant::Class { .. } => {
                            names.push(pool.class_name(*argument)?.to_owned());
                        }
                        other => {
                            return Err(VmError::BadConstantPool(format!(
                                "typeSwitch argument is {other:?}, expected a Class"
                            )));
                        }
                    }
                }
                Pending::TypeSwitch { names }
            }
            ("java/lang/runtime/ObjectMethods", "bootstrap") => {
                let kind = match name {
                    "toString" => RecordMethodKind::ToString,
                    "equals" => RecordMethodKind::Equals,
                    "hashCode" => RecordMethodKind::HashCode,
                    other => {
                        return Err(VmError::Unsupported {
                            feature: "ObjectMethods",
                            detail: format!("generated method {other}"),
                        });
                    }
                };
                // Arguments: Class, component names ("x;y"), then one field handle per component.
                let class_index = *bootstrap.arguments.first().ok_or_else(|| {
                    VmError::BadConstantPool("ObjectMethods has no class argument".to_owned())
                })?;
                let class_name = match pool.get(class_index)? {
                    Constant::Class { .. } => pool.class_name(class_index)?.to_owned(),
                    other => {
                        return Err(VmError::BadConstantPool(format!(
                            "ObjectMethods class argument is {other:?}"
                        )));
                    }
                };
                let names_index = *bootstrap.arguments.get(1).ok_or_else(|| {
                    VmError::BadConstantPool("ObjectMethods has no component list".to_owned())
                })?;
                let component_names = string_constant(pool, names_index)?
                    .split(';')
                    .map(str::to_owned)
                    .collect();
                Pending::RecordMethod {
                    kind,
                    class_name,
                    component_names,
                    handles: bootstrap.arguments[2..].to_vec(),
                }
            }
            _ => Pending::Unsupported(format!("{owner}.{method_name}")),
        };
        (descriptor, pending)
    };

    let site = match pending {
        Pending::Ready(site) => site,
        Pending::Unsupported(bootstrap) => CallSite::Unsupported { bootstrap },
        Pending::RecordMethod {
            kind,
            class_name,
            component_names,
            handles,
        } => {
            let record_class = shared.load_class(path, &class_name)?;
            let mut components = Vec::with_capacity(handles.len());
            for (index, handle) in handles.iter().enumerate() {
                let (_, field_name, descriptor) = handle_target(
                    shared
                        .class(record_class)
                        .file
                        .as_ref()
                        .map(|file| &file.constant_pool)
                        .ok_or_else(|| {
                            VmError::BadConstantPool(format!("{class_name} is synthetic"))
                        })?,
                    *handle,
                )?;
                let slot = shared
                    .class(record_class)
                    .field_slot(&field_name, &descriptor)
                    .ok_or_else(|| VmError::FieldNotFound {
                        class: class_name.clone(),
                        name: field_name.clone(),
                    })?;
                components.push(Component {
                    name: component_names.get(index).cloned().unwrap_or(field_name),
                    descriptor,
                    slot,
                });
            }
            CallSite::RecordMethod {
                kind,
                class_name,
                components,
            }
        }
        Pending::TypeSwitch { names } => {
            let mut cases = Vec::with_capacity(names.len());
            for name in names {
                cases.push(shared.load_class(path, &name)?);
            }
            CallSite::TypeSwitch { cases }
        }
        Pending::Lambda {
            owner,
            name,
            descriptor,
            sam_name,
            sam_descriptor,
        } => {
            let target_class = shared.load_class(path, &owner)?;
            let (target_class, target_method) = shared
                .resolve_method(target_class, &name, &descriptor)
                .ok_or(VmError::MethodNotFound {
                    class: owner,
                    name,
                    descriptor,
                })?;
            // The call site's name is the SAM name (javac puts it in the `invokedynamic`
            // NameAndType); the bootstrap's `samMethodType` gave the descriptor.
            CallSite::Lambda {
                target_class,
                target_method,
                sam_name,
                sam_descriptor,
            }
        }
    };
    Ok(CallSiteInfo { descriptor, site })
}

/// Walks `MethodHandle -> Methodref` to the `(owner, name)` the bootstrap method lives at.
pub(crate) fn bootstrap_target_name(
    pool: &ConstantPool,
    bootstrap: &BootstrapMethod,
) -> Result<(String, String), VmError> {
    bootstrap_target(pool, bootstrap)
}

fn bootstrap_target(
    pool: &ConstantPool,
    bootstrap: &BootstrapMethod,
) -> Result<(String, String), VmError> {
    let (owner, name, _) = handle_target(pool, bootstrap.method_ref)?;
    Ok((owner, name))
}

/// Resolves a `CONSTANT_MethodHandle` to `(owner, name, descriptor)` through its `Methodref`.
fn handle_target(
    pool: &ConstantPool,
    handle_index: u16,
) -> Result<(String, String, String), VmError> {
    let reference_index = match pool.get(handle_index)? {
        Constant::MethodHandle {
            reference_index, ..
        } => *reference_index,
        other => {
            return Err(VmError::BadConstantPool(format!(
                "entry {handle_index} is {other:?}, expected a MethodHandle"
            )));
        }
    };
    match pool.get(reference_index)? {
        Constant::MethodRef {
            class_index,
            name_and_type_index,
        }
        | Constant::InterfaceMethodRef {
            class_index,
            name_and_type_index,
        }
        | Constant::FieldRef {
            class_index,
            name_and_type_index,
        } => {
            let owner = pool.class_name(*class_index)?.to_owned();
            let (name, descriptor) = pool.name_and_type(*name_and_type_index)?;
            Ok((owner, name.to_owned(), descriptor.to_owned()))
        }
        other => Err(VmError::BadConstantPool(format!(
            "method handle points at {other:?}, expected a Methodref"
        ))),
    }
}

/// Reads `makeConcatWithConstants`'s recipe and its `\u0002` constants.
fn recipe_with_constants(
    pool: &ConstantPool,
    bootstrap: &BootstrapMethod,
) -> Result<Vec<RecipePart>, VmError> {
    let recipe_index = *bootstrap.arguments.first().ok_or_else(|| {
        VmError::BadConstantPool("makeConcatWithConstants has no recipe argument".to_owned())
    })?;
    let recipe = string_constant(pool, recipe_index)?;

    let mut constants = bootstrap.arguments[1..].iter();
    let mut parts = Vec::new();
    let mut text = String::new();

    for character in recipe.chars() {
        match character {
            '\u{1}' => {
                if !text.is_empty() {
                    parts.push(RecipePart::Text(std::mem::take(&mut text)));
                }
                parts.push(RecipePart::Argument);
            }
            '\u{2}' => {
                if !text.is_empty() {
                    parts.push(RecipePart::Text(std::mem::take(&mut text)));
                }
                let index = *constants.next().ok_or_else(|| {
                    VmError::BadConstantPool(
                        "the recipe asks for a constant but the bootstrap has none".to_owned(),
                    )
                })?;
                parts.push(RecipePart::Constant(string_constant(pool, index)?));
            }
            other => text.push(other),
        }
    }
    if !text.is_empty() {
        parts.push(RecipePart::Text(text));
    }
    Ok(parts)
}

fn string_constant(pool: &ConstantPool, index: u16) -> Result<String, VmError> {
    match pool.get(index)? {
        Constant::String { string_index } => Ok(pool.utf8(*string_index)?.to_owned()),
        Constant::Utf8(value) => Ok(value.clone()),
        other => Err(VmError::BadConstantPool(format!(
            "entry {index} is {other:?}, expected a string constant"
        ))),
    }
}
