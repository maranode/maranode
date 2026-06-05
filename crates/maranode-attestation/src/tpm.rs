//! read TPM 2.0 PCR values from /dev/tpmrm0 on Linux without a C library.
//! Sends TPM2_PCR_Read command (CC=0x17E) with SHA-256.
//! on other platforms or when device is missing, returns TpmResult::Unavailable.

use std::collections::BTreeMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TpmResult {
    /// PCR values as map from index (0-23) to hex string
    Available { pcrs: BTreeMap<u8, String> },
    /// TPM device exists but read command failed
    Error { reason: String },
    /// no TPM device found, or not running on Linux
    Unavailable,
}

pub fn read_pcrs() -> TpmResult {
    #[cfg(target_os = "linux")]
    {
        match try_read_pcrs_linux() {
            Ok(pcrs) => TpmResult::Available { pcrs },
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("No such file") || msg.contains("os error 2") {
                    TpmResult::Unavailable
                } else {
                    TpmResult::Error { reason: msg }
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    TpmResult::Unavailable
}

#[cfg(target_os = "linux")]
fn try_read_pcrs_linux() -> anyhow::Result<BTreeMap<u8, String>> {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;

    // Use /dev/tpmrm0 (resource manager) when present, else /dev/tpm0
    let dev = if std::path::Path::new("/dev/tpmrm0").exists() {
        "/dev/tpmrm0"
    } else {
        "/dev/tpm0"
    };

    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc_o_nonblock())
        .open(dev)?;

    let mut all_pcrs = BTreeMap::new();

    // many TPMs allow up to 8 PCRs per TPM2_PCR_Read call.
    // read three batches: indices 0-7, 8-15, 16-23.
    for batch_start in [0u8, 8, 16] {
        let select = pcr_select_mask(batch_start, batch_start + 8);
        let cmd = build_pcr_read_cmd(select);

        f.write_all(&cmd)?;

        let mut resp = vec![0u8; 4096];
        let n = f.read(&mut resp)?;
        resp.truncate(n);

        let batch = parse_pcr_read_response(&resp, batch_start)?;
        all_pcrs.extend(batch);
    }

    Ok(all_pcrs)
}

/// build TPM2_PCR_Read command bytes for the 3-byte PCR selection mask.
/// all multi-byte fields are big-endian per TPM2 specification.
fn build_pcr_read_cmd(pcr_select: [u8; 3]) -> Vec<u8> {
    // total size: header 10 bytes + pcrSelectionIn 10 bytes = 20 bytes
    let size: u32 = 20;
    let mut cmd = Vec::with_capacity(size as usize);

    // tag TPM_ST_NO_SESSIONS = 0x8001
    cmd.extend_from_slice(&0x8001u16.to_be_bytes());
    cmd.extend_from_slice(&size.to_be_bytes());
    // command code TPM_CC_PCR_Read = 0x0000017E
    cmd.extend_from_slice(&0x0000_017Eu32.to_be_bytes());
    // TPML_PCR_SELECTION count = 1
    cmd.extend_from_slice(&1u32.to_be_bytes());
    // Hash algorithm TPM_ALG_SHA256 = 0x000B
    cmd.extend_from_slice(&0x000Bu16.to_be_bytes());
    // sizeofSelect = 3 bytes
    cmd.push(3);
    // pcrSelect bitmask bytes
    cmd.extend_from_slice(&pcr_select);

    cmd
}

/// build 3-byte PCR select mask for indices from start up to (but not including) start+8.
fn pcr_select_mask(start: u8, end: u8) -> [u8; 3] {
    let mut mask = [0u8; 3];
    for i in start..end.min(24) {
        let byte = (i / 8) as usize;
        let bit = i % 8;
        if byte < 3 {
            mask[byte] |= 1 << bit;
        }
    }
    mask
}

/// parse TPM2_PCR_Read response byte
fn parse_pcr_read_response(
    resp: &[u8],
    batch_start: u8,
) -> anyhow::Result<BTreeMap<u8, String>> {
    if resp.len() < 10 {
        anyhow::bail!("response too short ({} bytes)", resp.len());
    }

    // offset 6-9: response code as big-endian u32
    let rc = u32::from_be_bytes(resp[6..10].try_into().unwrap());
    if rc != 0 {
        anyhow::bail!("TPM2_PCR_Read returned error code 0x{:08X}", rc);
    }

    // offset 10-13: pcrUpdateCounter (not used here)
    // offset 14-17: pcrSelectionOut.count
    if resp.len() < 18 {
        anyhow::bail!("response truncated before pcrSelectionOut");
    }
    let sel_count = u32::from_be_bytes(resp[14..18].try_into().unwrap());
    if sel_count == 0 {
        return Ok(BTreeMap::new());
    }

    // skip pcrSelectionOut entries; each entry is 2+1+sizeofSelect bytes
    let mut pos = 18usize;
    for _ in 0..sel_count {
        if pos + 6 > resp.len() {
            anyhow::bail!("truncated pcrSelectionOut");
        }
        let sizeof_select = resp[pos + 2] as usize;
        pos += 2 + 1 + sizeof_select;
    }

    // read digests.count field
    if pos + 4 > resp.len() {
        anyhow::bail!("truncated at digests.count");
    }
    let digest_count = u32::from_be_bytes(resp[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;

    let mut result = BTreeMap::new();
    for i in 0..digest_count {
        if pos + 2 > resp.len() {
            anyhow::bail!("truncated at digest size");
        }
        let digest_size = u16::from_be_bytes(resp[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;

        if pos + digest_size > resp.len() {
            anyhow::bail!("truncated digest data");
        }
        if digest_size == 32 {
            let pcr_index = batch_start + i as u8;
            let hex = hex::encode(&resp[pos..pos + 32]);
            result.insert(pcr_index, hex);
        }
        pos += digest_size;
    }

    Ok(result)
}

/// on Linux open TPM with O_NONBLOCK so read does not block forever.
#[cfg(target_os = "linux")]
fn libc_o_nonblock() -> i32 {
    // o_nonblock flag value on Linux x86-64
    0o4000
}
