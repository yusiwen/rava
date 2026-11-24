//! JVMS generic signatures as a typed model (JVMS §4.7.9.1).
//!
//! A class file keeps a generic signature as one string in a `Signature` attribute. That
//! string is a grammar of its own: `L...;` class types with optional type arguments,
//! `T...;` type variables, `[...` arrays, `+`/`-`/`*` type arguments, and a prefix of
//! `<...>` declarations with class and interface bounds. This module parses that grammar
//! into [`ClassSignature`], [`MethodSignature`] and [`TypeSignature`] values and reports a
//! malformed signature as a structured [`SignatureError`], never as a panic.
//!
//! Three properties are deliberate:
//!
//! - **Raw material preserved.** A type variable keeps its name and every bound in the
//!   order the class file encodes them, an owning type of a nested class type keeps its
//!   own type arguments, and a wildcard keeps `extends`/`super`/unbounded apart.
//! - **Reference types only where the grammar says so.** A field signature is a
//!   `ReferenceTypeSignature`; a method's return may be `V`, but `V` is not a field,
//!   parameter or array component.
//! - **Bounded recursion.** Nesting deeper than [`MAX_TYPE_DEPTH`] is a typed error, so a
//!   hostile signature cannot exhaust the host stack.
//!
//! Resolving these names to loaded classes and allocating the reflective `Type` objects is
//! the VM's job (`rava-vm`'s `generic` module); this crate only reads the grammar.

use std::fmt;

/// The deepest type nesting this parser accepts.
///
/// The limit counts class types, arrays, type-argument lists and type-variable bounds, so
/// a signature such as `[[[[I` or `LList<LList<...>>;` cannot recurse without bound.
pub const MAX_TYPE_DEPTH: usize = 256;

/// One base type of the JVMS `BaseType` production.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveSignature {
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
}

impl PrimitiveSignature {
    /// The one-character descriptor of the primitive (`I`, `Z`, ...).
    pub fn descriptor(self) -> char {
        match self {
            Self::Boolean => 'Z',
            Self::Byte => 'B',
            Self::Char => 'C',
            Self::Short => 'S',
            Self::Int => 'I',
            Self::Long => 'J',
            Self::Float => 'F',
            Self::Double => 'D',
        }
    }

    /// The Java source name of the primitive.
    pub fn name(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Byte => "byte",
            Self::Char => "char",
            Self::Short => "short",
            Self::Int => "int",
            Self::Long => "long",
            Self::Float => "float",
            Self::Double => "double",
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            b'Z' => Self::Boolean,
            b'B' => Self::Byte,
            b'C' => Self::Char,
            b'S' => Self::Short,
            b'I' => Self::Int,
            b'J' => Self::Long,
            b'F' => Self::Float,
            b'D' => Self::Double,
            _ => return None,
        })
    }
}

impl fmt::Display for PrimitiveSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A `ClassTypeSignature` (`Lpkg/Name<...>;`), with its nesting preserved.
///
/// `name` is the internal binary name of the type the signature denotation names: for
/// `Lpkg/Outer<TT;>.Inner<TU;>;` it is `pkg/Outer$Inner` and `owner` is the parsed
/// `pkg/Outer` type with its own `arguments`. A signature written without a suffix
/// (`Lpkg/Outer$Inner<...>;`) has no explicit owner even though `Outer$Inner` is a member
/// class; the VM derives the owner from the loaded class's `InnerClasses` entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassTypeSignature {
    /// The internal binary name (`java/util/Map$Entry`), never empty.
    pub name: String,
    /// Type arguments of the innermost type, empty when it is raw.
    pub arguments: Vec<TypeArgument>,
    /// The enclosing class type of a `.Inner` suffix, if the signature spells one.
    pub owner: Option<Box<ClassTypeSignature>>,
}

/// One `TypeArgument` of a parameterized class type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeArgument {
    /// A concrete reference type: `List<String>`'s `String`.
    Type(TypeSignature),
    /// `*`: an unbounded wildcard.
    Unbounded,
    /// `+ Type`: an upper-bounded wildcard (`? extends Type`).
    Extends(TypeSignature),
    /// `- Type`: a lower-bounded wildcard (`? super Type`).
    Super(TypeSignature),
}

/// A `JavaTypeSignature` or `ReferenceTypeSignature`: the type of a field, parameter,
/// return or array component.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeSignature {
    /// `B`, `C`, `D`, `F`, `I`, `J`, `S`, `Z`.
    Primitive(PrimitiveSignature),
    /// `L...;`.
    Class(ClassTypeSignature),
    /// `TName;`.
    TypeVariable(String),
    /// `[Type`.
    Array(Box<TypeSignature>),
}

/// One `<Name: Bound: InterfaceBound...>` declaration.
///
/// The grammar lets the class bound be empty (`<E::Ljava/lang/CharSequence;>`); the JDK
/// then reports the interface bounds only, without a synthetic `Object` bound, which is
/// why the class bound stays an `Option`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeParameter {
    pub name: String,
    pub class_bound: Option<TypeSignature>,
    pub interface_bounds: Vec<TypeSignature>,
}

impl TypeParameter {
    /// The bounds in declaration order: the class bound (when spelled) then the interface
    /// bounds. Empty means the type variable is unbounded.
    pub fn bounds(&self) -> impl Iterator<Item = &TypeSignature> {
        self.class_bound.iter().chain(self.interface_bounds.iter())
    }
}

/// A class signature: its type parameters, superclass and superinterfaces.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassSignature {
    pub type_parameters: Vec<TypeParameter>,
    pub superclass: ClassTypeSignature,
    pub interfaces: Vec<ClassTypeSignature>,
}

/// A method or constructor signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodSignature {
    pub type_parameters: Vec<TypeParameter>,
    pub parameters: Vec<TypeSignature>,
    /// `None` for a `V` result; every other result is a Java type.
    pub return_type: Option<TypeSignature>,
    /// The `^` throws signatures in encoded order.
    pub throws: Vec<TypeSignature>,
}

/// Why a `Signature` string could not be parsed.
///
/// The variants name the failure and the byte offset, so a caller can report a malformed
/// attribute without guessing which construct failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// The signature ended while a construct still needed input.
    UnexpectedEnd {
        offset: usize,
        expected: &'static str,
    },
    /// A byte that cannot appear at this position.
    UnexpectedCharacter {
        offset: usize,
        found: char,
        expected: &'static str,
    },
    /// A class or type-variable name was empty.
    EmptyIdentifier { offset: usize },
    /// Parsing finished with input left over.
    TrailingCharacters { offset: usize, remaining: usize },
    /// `V` appeared where only a reference or Java type is legal.
    UnexpectedVoid { offset: usize },
    /// Nesting exceeded [`MAX_TYPE_DEPTH`].
    TooDeep { limit: usize },
}

impl fmt::Display for SignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { offset, expected } => {
                write!(f, "unexpected end at byte {offset}: expected {expected}")
            }
            Self::UnexpectedCharacter {
                offset,
                found,
                expected,
            } => write!(
                f,
                "unexpected character {found:?} at byte {offset}: expected {expected}"
            ),
            Self::EmptyIdentifier { offset } => {
                write!(f, "empty identifier at byte {offset}")
            }
            Self::TrailingCharacters { offset, remaining } => {
                write!(f, "{remaining} trailing character(s) from byte {offset}")
            }
            Self::UnexpectedVoid { offset } => {
                write!(f, "void is not a legal type at byte {offset}")
            }
            Self::TooDeep { limit } => {
                write!(f, "signature nesting exceeds the limit of {limit}")
            }
        }
    }
}

impl std::error::Error for SignatureError {}

/// Parse a `ClassSignature` (`<T:...>SuperclassInterface*`).
pub fn parse_class_signature(text: &str) -> Result<ClassSignature, SignatureError> {
    let mut parser = Parser::new(text);
    let type_parameters = parser.type_parameters()?;
    let superclass = parser.class_type()?;
    let mut interfaces = Vec::new();
    while parser.peek().is_some() {
        interfaces.push(parser.class_type()?);
    }
    Ok(ClassSignature {
        type_parameters,
        superclass,
        interfaces,
    })
}

/// Parse a `MethodSignature` (`<T:...>(Params)Result^Throws*`).
pub fn parse_method_signature(text: &str) -> Result<MethodSignature, SignatureError> {
    let mut parser = Parser::new(text);
    let type_parameters = parser.type_parameters()?;
    parser.expect(b'(', "the '(' of a method signature")?;
    let mut parameters = Vec::new();
    while parser.peek() != Some(b')') {
        if parser.peek().is_none() {
            return Err(parser.unexpected_end("a parameter type or ')'"));
        }
        parameters.push(parser.java_type()?);
    }
    parser.bump();
    let return_type = if parser.peek() == Some(b'V') {
        parser.bump();
        None
    } else {
        Some(parser.java_type()?)
    };
    let mut throws = Vec::new();
    while parser.peek() == Some(b'^') {
        parser.bump();
        throws.push(match parser.peek() {
            Some(b'L') => TypeSignature::Class(parser.class_type()?),
            Some(b'T') => parser.type_variable()?,
            Some(byte) => return Err(parser.unexpected_character(byte, "a throws type signature")),
            None => return Err(parser.unexpected_end("a throws type signature")),
        });
    }
    parser.expect_end()?;
    Ok(MethodSignature {
        type_parameters,
        parameters,
        return_type,
        throws,
    })
}

/// Parse a `FieldSignature` (`ReferenceTypeSignature`).
///
/// A field signature cannot be a base type or `void`; the field's descriptor is the only
/// place a primitive type appears.
pub fn parse_field_signature(text: &str) -> Result<TypeSignature, SignatureError> {
    let mut parser = Parser::new(text);
    if parser.peek() == Some(b'V') {
        return Err(SignatureError::UnexpectedVoid {
            offset: parser.position(),
        });
    }
    let signature = parser.reference_type()?;
    parser.expect_end()?;
    Ok(signature)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    position: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            bytes: text.as_bytes(),
            position: 0,
            depth: 0,
        }
    }

    fn position(&self) -> usize {
        self.position
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }

    fn expect(&mut self, byte: u8, expected: &'static str) -> Result<(), SignatureError> {
        match self.peek() {
            Some(found) if found == byte => {
                self.position += 1;
                Ok(())
            }
            Some(found) => Err(self.unexpected_character(found, expected)),
            None => Err(self.unexpected_end(expected)),
        }
    }

    fn unexpected_end(&self, expected: &'static str) -> SignatureError {
        SignatureError::UnexpectedEnd {
            offset: self.position,
            expected,
        }
    }

    fn unexpected_character(&self, found: u8, expected: &'static str) -> SignatureError {
        let found = self.text[self.position..]
            .chars()
            .next()
            .unwrap_or_else(|| char::from(found));
        SignatureError::UnexpectedCharacter {
            offset: self.position,
            found,
            expected,
        }
    }

    fn enter(&mut self) -> Result<(), SignatureError> {
        if self.depth >= MAX_TYPE_DEPTH {
            return Err(SignatureError::TooDeep {
                limit: MAX_TYPE_DEPTH,
            });
        }
        self.depth += 1;
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn expect_end(&self) -> Result<(), SignatureError> {
        if self.position == self.bytes.len() {
            return Ok(());
        }
        Err(SignatureError::TrailingCharacters {
            offset: self.position,
            remaining: self.text[self.position..].chars().count(),
        })
    }

    /// Read an identifier up to one of `terminators`.
    fn identifier(
        &mut self,
        terminators: &[u8],
        _expected: &'static str,
    ) -> Result<String, SignatureError> {
        let start = self.position;
        while let Some(byte) = self.peek() {
            if terminators.contains(&byte) {
                break;
            }
            self.position += 1;
        }
        if start == self.position {
            return Err(SignatureError::EmptyIdentifier { offset: start });
        }
        // Every terminator is ASCII, so stopping at one always lands on a character
        // boundary and the slice is valid UTF-8.
        let identifier = &self.text[start..self.position];
        Ok(identifier.to_owned())
    }

    /// A class name segment may contain `/` and `$`; a type-variable name may not.
    fn class_identifier(&mut self) -> Result<String, SignatureError> {
        self.identifier(b";.<>:[", "a class name")
    }

    fn variable_identifier(&mut self) -> Result<String, SignatureError> {
        self.identifier(b";.<>/:[", "a type variable name")
    }

    fn type_parameters(&mut self) -> Result<Vec<TypeParameter>, SignatureError> {
        if self.peek() != Some(b'<') {
            return Ok(Vec::new());
        }
        self.enter()?;
        self.bump();
        let mut parameters = Vec::new();
        while self.peek() != Some(b'>') {
            if self.peek().is_none() {
                return Err(self.unexpected_end("'>' closing a type-parameter list"));
            }
            parameters.push(self.type_parameter()?);
        }
        if parameters.is_empty() {
            return Err(self.unexpected_character(b'>', "a type parameter"));
        }
        self.bump();
        self.leave();
        Ok(parameters)
    }

    fn type_parameter(&mut self) -> Result<TypeParameter, SignatureError> {
        let name = self.variable_identifier()?;
        self.expect(b':', "':' after a type-parameter name")?;
        let class_bound = match self.peek() {
            Some(b'L' | b'T' | b'[') => Some(self.reference_type()?),
            _ => None,
        };
        let mut interface_bounds = Vec::new();
        while self.peek() == Some(b':') {
            self.bump();
            interface_bounds.push(self.reference_type()?);
        }
        Ok(TypeParameter {
            name,
            class_bound,
            interface_bounds,
        })
    }

    fn class_type(&mut self) -> Result<ClassTypeSignature, SignatureError> {
        self.enter()?;
        self.expect(b'L', "'L' starting a class type")?;
        let name = self.class_identifier()?;
        let arguments = self.type_arguments()?;
        let mut current = ClassTypeSignature {
            name,
            arguments,
            owner: None,
        };
        while self.peek() == Some(b'.') {
            self.bump();
            let simple = self.class_identifier()?;
            let arguments = self.type_arguments()?;
            let outer = std::mem::replace(
                &mut current,
                ClassTypeSignature {
                    name: simple,
                    arguments,
                    owner: None,
                },
            );
            current.name = format!("{outer_name}${}", current.name, outer_name = outer.name);
            current.owner = Some(Box::new(outer));
        }
        self.expect(b';', "';' ending a class type")?;
        self.leave();
        Ok(current)
    }

    fn type_arguments(&mut self) -> Result<Vec<TypeArgument>, SignatureError> {
        if self.peek() != Some(b'<') {
            return Ok(Vec::new());
        }
        self.enter()?;
        self.bump();
        let mut arguments = Vec::new();
        while self.peek() != Some(b'>') {
            if self.peek().is_none() {
                return Err(self.unexpected_end("'>' closing a type-argument list"));
            }
            arguments.push(self.type_argument()?);
        }
        if arguments.is_empty() {
            return Err(self.unexpected_character(b'>', "a type argument"));
        }
        self.bump();
        self.leave();
        Ok(arguments)
    }

    fn type_argument(&mut self) -> Result<TypeArgument, SignatureError> {
        match self.peek() {
            Some(b'*') => {
                self.bump();
                Ok(TypeArgument::Unbounded)
            }
            Some(b'+') => {
                self.bump();
                Ok(TypeArgument::Extends(self.reference_type()?))
            }
            Some(b'-') => {
                self.bump();
                Ok(TypeArgument::Super(self.reference_type()?))
            }
            _ => Ok(TypeArgument::Type(self.reference_type()?)),
        }
    }

    fn reference_type(&mut self) -> Result<TypeSignature, SignatureError> {
        match self.peek() {
            Some(b'L') => Ok(TypeSignature::Class(self.class_type()?)),
            Some(b'T') => self.type_variable(),
            Some(b'[') => {
                self.enter()?;
                self.bump();
                let component = self.java_type()?;
                self.leave();
                Ok(TypeSignature::Array(Box::new(component)))
            }
            Some(b'V') => Err(SignatureError::UnexpectedVoid {
                offset: self.position,
            }),
            Some(found) => Err(self.unexpected_character(found, "a reference type signature")),
            None => Err(self.unexpected_end("a reference type signature")),
        }
    }

    fn type_variable(&mut self) -> Result<TypeSignature, SignatureError> {
        self.expect(b'T', "'T' starting a type variable")?;
        let name = self.variable_identifier()?;
        self.expect(b';', "';' ending a type variable")?;
        Ok(TypeSignature::TypeVariable(name))
    }

    fn java_type(&mut self) -> Result<TypeSignature, SignatureError> {
        if let Some(primitive) = self.peek().and_then(PrimitiveSignature::from_byte) {
            self.bump();
            return Ok(TypeSignature::Primitive(primitive));
        }
        self.reference_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_signatures_preserve_arguments_and_owners() {
        let signature = parse_field_signature("Ljava/util/List<Ljava/lang/String;>;").unwrap();
        let TypeSignature::Class(class) = signature else {
            panic!("expected a class type");
        };
        assert_eq!(class.name, "java/util/List");
        assert_eq!(
            class.arguments,
            vec![TypeArgument::Type(TypeSignature::Class(
                ClassTypeSignature {
                    name: "java/lang/String".to_owned(),
                    arguments: Vec::new(),
                    owner: None,
                }
            ))]
        );
        assert_eq!(class.owner, None);

        let nested =
            parse_field_signature("Ldemo/Outer<Ljava/lang/String;>.Inner<Ljava/lang/Integer;>;")
                .unwrap();
        let TypeSignature::Class(inner) = nested else {
            panic!("expected a class type");
        };
        assert_eq!(inner.name, "demo/Outer$Inner");
        assert_eq!(inner.arguments.len(), 1);
        let owner = inner.owner.expect("the nested type keeps its owner");
        assert_eq!(owner.name, "demo/Outer");
        assert_eq!(owner.arguments.len(), 1);
    }

    #[test]
    fn field_signatures_distinguish_wildcards() {
        let unbounded = parse_field_signature("Ljava/util/List<*>;").unwrap();
        let bounded = parse_field_signature("Ljava/util/List<+Ljava/lang/Number;>;").unwrap();
        let lower = parse_field_signature("Ljava/util/List<-Ljava/lang/Integer;>;").unwrap();
        let arguments = |signature| match signature {
            TypeSignature::Class(class) => class.arguments,
            _ => panic!("expected a class type"),
        };
        assert_eq!(arguments(unbounded), vec![TypeArgument::Unbounded]);
        assert!(matches!(
            arguments(bounded)[0],
            TypeArgument::Extends(TypeSignature::Class(_))
        ));
        assert!(matches!(
            arguments(lower)[0],
            TypeArgument::Super(TypeSignature::Class(_))
        ));

        let array = parse_field_signature("[TT;").unwrap();
        assert!(matches!(array, TypeSignature::Array(component) if
            matches!(*component, TypeSignature::TypeVariable(ref name) if name == "T")));
        assert_eq!(
            parse_field_signature("[[I").unwrap(),
            TypeSignature::Array(Box::new(TypeSignature::Array(Box::new(
                TypeSignature::Primitive(PrimitiveSignature::Int)
            ))))
        );
    }

    #[test]
    fn class_signatures_preserve_type_parameters_and_bounds() {
        let signature = parse_class_signature(
            "<T:Ljava/lang/Number;>Ljava/lang/Object;Ljava/lang/Comparable<Ldemo/Box<TT;>;>;",
        )
        .unwrap();
        assert_eq!(signature.type_parameters.len(), 1);
        assert_eq!(signature.type_parameters[0].name, "T");
        assert_eq!(
            signature.type_parameters[0].class_bound,
            Some(TypeSignature::Class(ClassTypeSignature {
                name: "java/lang/Number".to_owned(),
                arguments: Vec::new(),
                owner: None,
            }))
        );
        assert_eq!(signature.superclass.name, "java/lang/Object");
        assert_eq!(signature.interfaces.len(), 1);
        assert_eq!(signature.interfaces[0].name, "java/lang/Comparable");

        let multiple = parse_class_signature(
            "<A:Ljava/lang/Object;B::Ljava/lang/Comparable<TA;>;:Ljava/io/Serializable;>Ljava/lang/Object;",
        )
        .unwrap();
        assert_eq!(multiple.type_parameters.len(), 2);
        assert_eq!(multiple.type_parameters[1].name, "B");
        assert_eq!(multiple.type_parameters[1].class_bound, None);
        assert_eq!(multiple.type_parameters[1].interface_bounds.len(), 2);
    }

    #[test]
    fn method_signatures_preserve_parameters_return_and_throws() {
        let signature = parse_method_signature(
            "<E::Ljava/lang/CharSequence;>(Ljava/util/List<TE;>;)Ljava/util/List<TE;>;^Ljava/io/IOException;",
        )
        .unwrap();
        assert_eq!(signature.type_parameters.len(), 1);
        assert_eq!(signature.type_parameters[0].name, "E");
        assert_eq!(signature.parameters.len(), 1);
        assert_eq!(
            signature.return_type,
            Some(TypeSignature::Class(ClassTypeSignature {
                name: "java/util/List".to_owned(),
                arguments: vec![TypeArgument::Type(TypeSignature::TypeVariable(
                    "E".to_owned()
                ))],
                owner: None,
            }))
        );
        assert_eq!(signature.throws.len(), 1);

        let void = parse_method_signature("(I)V").unwrap();
        assert_eq!(void.return_type, None);
        assert_eq!(void.parameters.len(), 1);
    }

    #[test]
    fn malformed_signatures_are_typed_errors() {
        assert_eq!(
            parse_class_signature("<T"),
            Err(SignatureError::UnexpectedEnd {
                offset: 2,
                expected: "':' after a type-parameter name",
            })
        );
        assert!(matches!(
            parse_class_signature("Lfoo"),
            Err(SignatureError::UnexpectedEnd { offset: 4, .. })
        ));
        assert!(matches!(
            parse_field_signature("Ljava/util/List<;"),
            Err(SignatureError::UnexpectedCharacter { offset: 16, .. })
        ));
        assert!(matches!(
            parse_field_signature("I"),
            Err(SignatureError::UnexpectedCharacter { offset: 0, .. })
        ));
        assert!(matches!(
            parse_field_signature("V"),
            Err(SignatureError::UnexpectedVoid { offset: 0 })
        ));
        assert!(matches!(
            parse_field_signature("Ljava/lang/String;;"),
            Err(SignatureError::TrailingCharacters { offset: 18, .. })
        ));
        assert!(matches!(
            parse_method_signature("(I"),
            Err(SignatureError::UnexpectedEnd { .. })
        ));
        assert!(matches!(
            parse_method_signature("()X"),
            Err(SignatureError::UnexpectedCharacter { offset: 2, .. })
        ));
    }

    #[test]
    fn nesting_is_bounded() {
        let deep = format!("{}I", "[".repeat(MAX_TYPE_DEPTH + 1));
        assert_eq!(
            parse_field_signature(&deep),
            Err(SignatureError::TooDeep {
                limit: MAX_TYPE_DEPTH
            })
        );
    }
}
