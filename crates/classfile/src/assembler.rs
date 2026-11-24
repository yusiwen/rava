//! A tiny instruction assembler for test fixtures.
//!
//! [`ClassBuilder`](crate::ClassBuilder) owns constant-pool indices; this owns bytecode offsets.
//! The two failure modes left in a hand-assembled fixture were a branch offset computed against the
//! wrong origin and an operand written in the wrong width — labels and a single `finish` pass remove
//! both, for the opcodes implemented here.
//!
//! What it covers is exactly what the fixtures need: integer constants, integer and
//! reference loads and stores, `iinc`, `aconst_null`, `dup`, two conditional branches,
//! `if_acmpeq`/`if_acmpne`, `goto`, `getstatic`, `getfield`, `putfield`, `new`, array
//! creation and access (`newarray`, `anewarray`, `multianewarray`, `arraylength`,
//! `iaload`/`iastore` and `aaload`/`aastore`), `ldc`/`ldc_w`, `invokevirtual`,
//! `invokeinterface`, `invokespecial`, `invokestatic` through [`op`](Assembler::op),
//! `athrow`, and the
//! `ireturn`/`areturn`/`return` forms. Everything else is still raw bytes through
//! [`Assembler::op`].
//!
//! `tableswitch` and `lookupswitch` are deliberately absent: their padding depends on the switch
//! opcode's own address, which is a different encoding problem (and the decoder already handles the
//! result).

use crate::Error;

/// A jump destination, bound by [`Assembler::place`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label(usize);

/// One 16-bit branch offset waiting for its label’s address.
struct Fixup {
    /// Offset of the branch's *opcode*, which is what the relative offset is measured from.
    opcode: usize,
    label: Label,
}

/// Bytecode under construction.
pub struct Assembler {
    bytes: Vec<u8>,
    labels: Vec<Option<usize>>,
    fixups: Vec<Fixup>,
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            labels: Vec::new(),
            fixups: Vec::new(),
        }
    }

    /// Creates a label; bind it later with [`place`](Self::place).
    pub fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    /// Binds `label` to the current offset.
    pub fn place(&mut self, label: Label) {
        self.labels[label.0] = Some(self.bytes.len());
    }

    /// The byte offset `label` currently names, or `None` while it is unbound.
    ///
    /// A `Code` exception table names byte offsets rather than labels, so a fixture that
    /// uses labels for handler ranges reads them back here before
    /// [`finish`](Self::finish) consumes the assembler.
    pub fn offset(&self, label: Label) -> Option<usize> {
        self.labels[label.0]
    }

    /// Emits a raw opcode and operands, for anything without a helper.
    pub fn op(&mut self, opcode: u8, operands: &[u8]) {
        self.bytes.push(opcode);
        self.bytes.extend_from_slice(operands);
    }

    /// `iconst_<n>` for -1..=5, `bipush` for the next range, `sipush` beyond it.
    pub fn iconst(&mut self, value: i32) {
        match value {
            // `iconst_m1` is 0x02, `iconst_0` is 0x03, ... `iconst_5` is 0x08.
            -1 => self.bytes.push(0x02),
            0..=5 => self.bytes.push(0x03 + value as u8),
            -128..=127 => {
                self.bytes.push(0x10);
                self.bytes.push(value as u8);
            }
            _ => {
                self.bytes.push(0x11);
                self.bytes.extend_from_slice(&(value as i16).to_be_bytes());
            }
        }
    }

    /// `aconst_null`: push the null reference.
    pub fn aconst_null(&mut self) {
        self.bytes.push(0x01);
    }

    /// `dup`: duplicate the top operand-stack value.
    pub fn dup(&mut self) {
        self.bytes.push(0x59);
    }

    pub fn iload(&mut self, slot: u8) {
        self.op(0x15, &[slot]);
    }

    /// `lload` for a long local.
    pub fn lload(&mut self, slot: u8) {
        match slot {
            0..=3 => self.bytes.push(0x1e + slot),
            _ => self.op(0x16, &[slot]),
        }
    }

    /// `fload` for a float local.
    pub fn fload(&mut self, slot: u8) {
        match slot {
            0..=3 => self.bytes.push(0x22 + slot),
            _ => self.op(0x17, &[slot]),
        }
    }

    /// `dload` for a double local.
    pub fn dload(&mut self, slot: u8) {
        match slot {
            0..=3 => self.bytes.push(0x26 + slot),
            _ => self.op(0x18, &[slot]),
        }
    }

    /// `aload` for a reference local. Slots 0..=3 use the implicit one-byte form.
    pub fn aload(&mut self, slot: u8) {
        match slot {
            0..=3 => self.bytes.push(0x2a + slot),
            _ => self.op(0x19, &[slot]),
        }
    }

    pub fn istore(&mut self, slot: u8) {
        self.op(0x36, &[slot]);
    }

    /// `astore` for a reference local. Slots 0..=3 use the implicit one-byte form.
    pub fn astore(&mut self, slot: u8) {
        match slot {
            0..=3 => self.bytes.push(0x4b + slot),
            _ => self.op(0x3a, &[slot]),
        }
    }

    pub fn iinc(&mut self, slot: u8, increment: i8) {
        self.op(0x84, &[slot, increment as u8]);
    }

    pub fn iadd(&mut self) {
        self.bytes.push(0x60);
    }

    /// `imul`: multiply two ints.
    pub fn imul(&mut self) {
        self.bytes.push(0x68);
    }

    /// `ldc`: push a constant-pool entry. Indices above 255 use the two-byte `ldc_w`
    /// form, which JVMS requires and the decoder accepts for either width.
    pub fn ldc(&mut self, index: u16) {
        if index < 256 {
            self.op(0x12, &[index as u8]);
        } else {
            self.ldc_w(index);
        }
    }

    /// `ldc_w`: push a constant-pool entry, always with the two-byte index form.
    pub fn ldc_w(&mut self, index: u16) {
        self.op(0x13, &index.to_be_bytes());
    }

    pub fn getstatic(&mut self, index: u16) {
        self.op(0xb2, &index.to_be_bytes());
    }

    /// `getfield`: read an instance field through a `CONSTANT_Fieldref`.
    pub fn getfield(&mut self, index: u16) {
        self.op(0xb4, &index.to_be_bytes());
    }

    /// `putfield`: write an instance field through a `CONSTANT_Fieldref`.
    pub fn putfield(&mut self, index: u16) {
        self.op(0xb5, &index.to_be_bytes());
    }

    /// `newarray`: allocate a primitive array. `atype` is the JVMS array-type code
    /// (`4` boolean, `5` char, `6` float, `7` double, `8` byte, `9` short, `10` int,
    /// `11` long).
    pub fn newarray(&mut self, atype: u8) {
        self.op(0xbc, &[atype]);
    }

    /// `anewarray`: allocate a reference array whose component is named by the
    /// `CONSTANT_Class` at `index`.
    pub fn anewarray(&mut self, index: u16) {
        self.op(0xbd, &index.to_be_bytes());
    }

    /// `multianewarray`: allocate `dimensions` levels of the array type named by the
    /// `CONSTANT_Class` at `index`.
    pub fn multianewarray(&mut self, index: u16, dimensions: u8) {
        let mut operands = index.to_be_bytes().to_vec();
        operands.push(dimensions);
        self.op(0xc5, &operands);
    }

    /// `arraylength`: push the length of an array reference.
    pub fn arraylength(&mut self) {
        self.bytes.push(0xbe);
    }

    /// `iaload`: read an `int` array element.
    pub fn iaload(&mut self) {
        self.bytes.push(0x2e);
    }

    /// `iastore`: write an `int` array element.
    pub fn iastore(&mut self) {
        self.bytes.push(0x4f);
    }

    /// `aaload`: read a reference array element.
    pub fn aaload(&mut self) {
        self.bytes.push(0x32);
    }

    /// `aastore`: write a reference array element.
    pub fn aastore(&mut self) {
        self.bytes.push(0x53);
    }

    /// `new`: allocate an instance of the class named by a `CONSTANT_Class`.
    ///
    /// Named `new_instance` because [`Assembler::new`] is the constructor.
    pub fn new_instance(&mut self, index: u16) {
        self.op(0xbb, &index.to_be_bytes());
    }

    /// `athrow`: throw the reference on top of the operand stack.
    pub fn athrow(&mut self) {
        self.bytes.push(0xbf);
    }

    pub fn invokevirtual(&mut self, index: u16) {
        self.op(0xb6, &index.to_be_bytes());
    }

    /// `invokeinterface`: call an interface method through a
    /// `CONSTANT_InterfaceMethodref`. `count` is the argument-slot count including the
    /// receiver and the reserved fourth byte is zero (JVMS §6.5.invokeinterface).
    pub fn invokeinterface(&mut self, index: u16, count: u8) {
        let [high, low] = index.to_be_bytes();
        self.op(0xb9, &[high, low, count, 0]);
    }

    /// `invokespecial`: constructors, private methods and superclass calls.
    pub fn invokespecial(&mut self, index: u16) {
        self.op(0xb7, &index.to_be_bytes());
    }

    /// `invokedynamic`: a call site through a `CONSTANT_InvokeDynamic`. The two reserved
    /// operand bytes are zero, as JVMS §6.5.invokedynamic requires.
    pub fn invokedynamic(&mut self, index: u16) {
        let [high, low] = index.to_be_bytes();
        self.op(0xba, &[high, low, 0, 0]);
    }

    /// `ifeq`, `ifne`, `iflt`, `ifge`, `ifgt`, `ifle` by their JVMS opcodes.
    pub fn if_cond(&mut self, opcode: u8, label: Label) {
        self.branch(opcode, label);
    }

    /// `if_icmpeq`..`if_icmple` (0x9f..=0xa4) by their JVMS opcodes.
    pub fn if_icmp(&mut self, opcode: u8, label: Label) {
        self.branch(opcode, label);
    }

    /// `if_acmpeq` (0xa5) or `if_acmpne` (0xa6): compare two references.
    pub fn if_acmp(&mut self, opcode: u8, label: Label) {
        self.branch(opcode, label);
    }

    pub fn goto(&mut self, label: Label) {
        self.branch(0xa7, label);
    }

    pub fn ireturn(&mut self) {
        self.bytes.push(0xac);
    }

    pub fn areturn(&mut self) {
        self.bytes.push(0xb0);
    }

    pub fn return_(&mut self) {
        self.bytes.push(0xb1);
    }

    /// Resolves every branch and returns the bytes.
    pub fn finish(self) -> Result<Vec<u8>, Error> {
        let mut bytes = self.bytes;
        for fixup in &self.fixups {
            let target = self.labels[fixup.label.0].ok_or(Error::InvalidAssembly {
                detail: format!("label {} was never placed", fixup.label.0),
            })?;
            // A branch offset is relative to the opcode's own address (JVMS §6.5).
            let offset = target as isize - fixup.opcode as isize;
            let offset = i16::try_from(offset).map_err(|_| Error::InvalidAssembly {
                detail: format!(
                    "branch to label {} is {offset} bytes away, beyond a signed 16-bit offset",
                    fixup.label.0
                ),
            })?;
            bytes[fixup.opcode + 1..fixup.opcode + 3].copy_from_slice(&offset.to_be_bytes());
        }
        Ok(bytes)
    }

    fn branch(&mut self, opcode: u8, label: Label) {
        let opcode_offset = self.bytes.len();
        self.bytes.push(opcode);
        self.bytes.extend_from_slice(&[0, 0]); // placeholder
        self.fixups.push(Fixup {
            opcode: opcode_offset,
            label,
        });
    }
}
