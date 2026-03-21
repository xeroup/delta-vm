// .dc (delta compiled) binary file format
//
// layout:
//   [header: 8 bytes]
//   [section*]
//
// header:
//   [magic: 4 bytes] = b"DC\x00\x01"
//   [version: u16 le]
//   [section_count: u16 le]
//
// section:
//   [tag: u8]
//   [len: u32 le]   - byte length of data
//   [data: len bytes]
//
// section tags:
//   0x01  FUNCS   - function table (metadata)
//   0x02  CODE    - bytecode for all functions (concatenated)
//   0x03  DATA    - constant data (strings, ints, floats)
//   0x04  EXTERNS - extern symbol table



pub const MAGIC: [u8; 4] = [b'D', b'C', 0x00, 0x01];
pub const VERSION: u16 = 1;

pub const TAG_FUNCS: u8 = 0x01;
pub const TAG_CODE: u8 = 0x02;
pub const TAG_DATA: u8 = 0x03;
pub const TAG_EXTERNS: u8 = 0x04;

// one entry in the FUNCS section
#[derive(Debug, Clone)]
pub struct FuncEntry {
    // byte offset into CODE section where this function starts
    pub code_offset: u32,
    // byte length of this function's code
    pub code_len: u32,
    // number of registers (params + locals)
    pub reg_count: u8,
    // number of params
    pub param_count: u8,
    // name length + name bytes (for debug/FFI lookup)
    pub name: String,
}

impl FuncEntry {
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.code_offset.to_le_bytes());
        buf.extend_from_slice(&self.code_len.to_le_bytes());
        buf.push(self.reg_count);
        buf.push(self.param_count);
        let name_bytes = self.name.as_bytes();
        buf.push(name_bytes.len() as u8);
        buf.extend_from_slice(name_bytes);
    }

    pub fn decode(data: &[u8], pos: usize) -> Option<(FuncEntry, usize)> {
        let code_offset = u32::from_le_bytes(data.get(pos..pos + 4)?.try_into().ok()?);
        let code_len = u32::from_le_bytes(data.get(pos + 4..pos + 8)?.try_into().ok()?);
        let reg_count = *data.get(pos + 8)?;
        let param_count = *data.get(pos + 9)?;
        let name_len = *data.get(pos + 10)? as usize;
        let name = String::from_utf8(data.get(pos + 11..pos + 11 + name_len)?.to_vec()).ok()?;
        Some((FuncEntry { code_offset, code_len, reg_count, param_count, name }, pos + 11 + name_len))
    }
}

// one entry in the EXTERNS section
#[derive(Debug, Clone)]
pub struct ExternEntry {
    pub name: String,
    pub param_count: u8,
    pub variadic: bool,
}

impl ExternEntry {
    pub fn encode(&self, buf: &mut Vec<u8>) {
        let name_bytes = self.name.as_bytes();
        buf.push(name_bytes.len() as u8);
        buf.extend_from_slice(name_bytes);
        // high bit of param_count encodes variadic flag
        let byte = if self.variadic { self.param_count | 0x80 } else { self.param_count };
        buf.push(byte);
    }

    pub fn decode(data: &[u8], pos: usize) -> Option<(ExternEntry, usize)> {
        let name_len = *data.get(pos)? as usize;
        let name = String::from_utf8(data.get(pos + 1..pos + 1 + name_len)?.to_vec()).ok()?;
        let byte = *data.get(pos + 1 + name_len)?;
        let variadic = (byte & 0x80) != 0;
        let param_count = byte & 0x7F;
        Some((ExternEntry { name, param_count, variadic }, pos + 2 + name_len))
    }
}

// one item in the DATA section
#[derive(Debug, Clone)]
pub enum DataEntry {
    // null-terminated string bytes
    Str(Vec<u8>),
    // raw i64 little-endian
    Int(i64),
    // raw f64 little-endian
    Float(f64),
}

impl DataEntry {
    pub fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            DataEntry::Str(bytes) => {
                buf.push(0x01);
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
            DataEntry::Int(n) => {
                buf.push(0x02);
                buf.extend_from_slice(&n.to_le_bytes());
            }
            DataEntry::Float(f) => {
                buf.push(0x03);
                buf.extend_from_slice(&f.to_bits().to_le_bytes());
            }
        }
    }

    pub fn decode(data: &[u8], pos: usize) -> Option<(DataEntry, usize)> {
        match data.get(pos)? {
            0x01 => {
                let len = u32::from_le_bytes(data.get(pos + 1..pos + 5)?.try_into().ok()?) as usize;
                let bytes = data.get(pos + 5..pos + 5 + len)?.to_vec();
                Some((DataEntry::Str(bytes), pos + 5 + len))
            }
            0x02 => {
                let n = i64::from_le_bytes(data.get(pos + 1..pos + 9)?.try_into().ok()?);
                Some((DataEntry::Int(n), pos + 9))
            }
            0x03 => {
                let bits = u64::from_le_bytes(data.get(pos + 1..pos + 9)?.try_into().ok()?);
                Some((DataEntry::Float(f64::from_bits(bits)), pos + 9))
            }
            _ => None,
        }
    }
}

// the complete in-memory representation of a .dc file
#[derive(Debug, Default)]
pub struct DcFile {
    pub funcs: Vec<FuncEntry>,
    pub code: Vec<u8>,
    pub data: Vec<DataEntry>,
    pub externs: Vec<ExternEntry>,
}

impl DcFile {
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // build sections
        let funcs_bytes = self.encode_funcs();
        let data_bytes = self.encode_data();
        let externs_bytes = self.encode_externs();

        // count non-empty sections
        let mut section_count: u16 = 0;
        if !funcs_bytes.is_empty() { section_count += 1; }
        if !self.code.is_empty() { section_count += 1; }
        if !data_bytes.is_empty() { section_count += 1; }
        if !externs_bytes.is_empty() { section_count += 1; }

        // header
        buf.extend_from_slice(&MAGIC);
        buf.extend_from_slice(&VERSION.to_le_bytes());
        buf.extend_from_slice(&section_count.to_le_bytes());

        // sections
        write_section(&mut buf, TAG_FUNCS, &funcs_bytes);
        write_section(&mut buf, TAG_CODE, &self.code);
        write_section(&mut buf, TAG_DATA, &data_bytes);
        write_section(&mut buf, TAG_EXTERNS, &externs_bytes);

        buf
    }

    pub fn deserialize(data: &[u8]) -> Option<DcFile> {
        // check magic
        if data.get(0..4)? != MAGIC {
            return None;
        }
        let _version = u16::from_le_bytes(data.get(4..6)?.try_into().ok()?);
        let section_count = u16::from_le_bytes(data.get(6..8)?.try_into().ok()?);

        let mut dc = DcFile::default();
        let mut pos = 8;

        for _ in 0..section_count {
            let tag = *data.get(pos)?;
            let len = u32::from_le_bytes(data.get(pos + 1..pos + 5)?.try_into().ok()?) as usize;
            let section_data = data.get(pos + 5..pos + 5 + len)?;
            pos += 5 + len;

            match tag {
                TAG_FUNCS => dc.funcs = decode_funcs(section_data),
                TAG_CODE => dc.code = section_data.to_vec(),
                TAG_DATA => dc.data = decode_data(section_data),
                TAG_EXTERNS => dc.externs = decode_externs(section_data),
                _ => {} // unknown section - skip for forward compat
            }
        }

        Some(dc)
    }

    fn encode_funcs(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.funcs.len() as u32).to_le_bytes());
        for f in &self.funcs {
            f.encode(&mut buf);
        }
        buf
    }

    fn encode_data(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        for d in &self.data {
            d.encode(&mut buf);
        }
        buf
    }

    fn encode_externs(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.externs.len() as u32).to_le_bytes());
        for e in &self.externs {
            e.encode(&mut buf);
        }
        buf
    }
}

fn write_section(buf: &mut Vec<u8>, tag: u8, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    buf.push(tag);
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);
}

fn decode_funcs(data: &[u8]) -> Vec<FuncEntry> {
    let mut out = Vec::new();
    if data.len() < 4 { return out; }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut pos = 4;
    for _ in 0..count {
        if let Some((entry, next)) = FuncEntry::decode(data, pos) {
            out.push(entry);
            pos = next;
        } else {
            break;
        }
    }
    out
}

fn decode_data(data: &[u8]) -> Vec<DataEntry> {
    let mut out = Vec::new();
    if data.len() < 4 { return out; }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut pos = 4;
    for _ in 0..count {
        if let Some((entry, next)) = DataEntry::decode(data, pos) {
            out.push(entry);
            pos = next;
        } else {
            break;
        }
    }
    out
}

fn decode_externs(data: &[u8]) -> Vec<ExternEntry> {
    let mut out = Vec::new();
    if data.len() < 4 { return out; }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut pos = 4;
    for _ in 0..count {
        if let Some((entry, next)) = ExternEntry::decode(data, pos) {
            out.push(entry);
            pos = next;
        } else {
            break;
        }
    }
    out
}
