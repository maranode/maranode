use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub fn read_context_length(path: &Path) -> Option<u32> {
    let mut f = std::fs::File::open(path).ok()?;
    read_context_length_from(&mut f).ok().flatten()
}

fn read_context_length_from<R: Read + Seek>(r: &mut R) -> io::Result<Option<u32>> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Ok(None);
    }

    let version = read_u32(r)?;
    if version == 0 || version > 3 {
        return Ok(None);
    }

    // v1 uses u32 counts, v2+ uses u64
    let n_kv: u64 = if version == 1 {
        let _n_tensors = read_u32(r)?;
        read_u32(r)? as u64
    } else {
        let _n_tensors = read_u64(r)?;
        read_u64(r)?
    };

    for _ in 0..n_kv {
        let key = read_string(r, version)?;
        let val_type = read_u32(r)?;

        if key == "llama.context_length" && val_type == 4 {
            return Ok(Some(read_u32(r)?));
        }

        skip_value(r, val_type, version)?;
    }

    Ok(None)
}

fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_string<R: Read>(r: &mut R, version: u32) -> io::Result<String> {
    let len: u64 = if version == 1 {
        read_u32(r)? as u64
    } else {
        read_u64(r)?
    };
    if len > 4096 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "key too long"));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

// skip a metadata value without interpreting it
fn skip_value<R: Read + Seek>(r: &mut R, val_type: u32, version: u32) -> io::Result<()> {
    match val_type {
        0 | 7 => { r.seek(SeekFrom::Current(1))?; }  // u8 / bool
        1     => { r.seek(SeekFrom::Current(1))?; }  // i8
        2 | 3 => { r.seek(SeekFrom::Current(2))?; }  // u16 / i16
        4 | 5 | 6 => { r.seek(SeekFrom::Current(4))?; }  // u32 / i32 / f32
        8     => { read_string(r, version)?; }
        9     => skip_array(r, version)?,
        10 | 11 | 12 => { r.seek(SeekFrom::Current(8))?; }  // u64 / i64 / f64
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown gguf value type")),
    }
    Ok(())
}

fn skip_array<R: Read + Seek>(r: &mut R, version: u32) -> io::Result<()> {
    let elem_type = read_u32(r)?;
    let count: u64 = if version == 1 {
        read_u32(r)? as u64
    } else {
        read_u64(r)?
    };
    for _ in 0..count {
        skip_value(r, elem_type, version)?;
    }
    Ok(())
}
