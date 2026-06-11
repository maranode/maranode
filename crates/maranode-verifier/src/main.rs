use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use ed25519_dalek::{Signature, VerifyingKey};
use maranode_common::receipt::InferenceReceipt;
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let opts = match parse_args(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    let json = match read_receipt(&opts.receipt_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading receipt: {e}");
            return ExitCode::FAILURE;
        }
    };

    let receipt: InferenceReceipt = match serde_json::from_str(&json) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error parsing receipt JSON: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut ok = true;

    match verify_signature(&receipt) {
        Ok(()) => println!("signature  OK  (ed25519, key {})", &receipt.signing_key_id[..16]),
        Err(e) => {
            println!("signature  FAIL  {e}");
            ok = false;
        }
    }

    if let Some(input_path) = &opts.input_path {
        match check_file_hash(input_path, &receipt.input_sha256) {
            Ok(()) => println!("input hash OK"),
            Err(e) => {
                println!("input hash FAIL  {e}");
                ok = false;
            }
        }
    }

    if let Some(output_path) = &opts.output_path {
        match check_file_hash(output_path, &receipt.output_sha256) {
            Ok(()) => println!("output hash OK"),
            Err(e) => {
                println!("output hash FAIL  {e}");
                ok = false;
            }
        }
    }

    println!();
    if ok {
        println!("VERIFIED");
        ExitCode::SUCCESS
    } else {
        println!("FAILED");
        ExitCode::FAILURE
    }
}

fn verify_signature(receipt: &InferenceReceipt) -> Result<(), String> {
    let sig_hex = receipt
        .signature
        .as_deref()
        .ok_or("receipt has no signature field")?;

    let sig_bytes = hex::decode(sig_hex).map_err(|e| format!("invalid signature hex: {e}"))?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| format!("invalid signature: {e}"))?;

    let key_bytes =
        hex::decode(&receipt.signing_key_id).map_err(|e| format!("invalid key hex: {e}"))?;
    let key_arr: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| "signing_key_id must be 32 bytes (64 hex chars)")?;
    let vk = VerifyingKey::from_bytes(&key_arr).map_err(|e| format!("invalid public key: {e}"))?;

    let msg = receipt.canonical_bytes();
    use ed25519_dalek::Verifier;
    vk.verify(&msg, &sig).map_err(|_| "signature mismatch".to_string())
}

fn check_file_hash(path: &PathBuf, expected: &str) -> Result<(), String> {
    let data =
        std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let got = format!("{:x}", Sha256::digest(&data));
    if got == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {} got {} ({})",
            expected,
            got,
            path.display()
        ))
    }
}

fn read_receipt(path: &Option<PathBuf>) -> Result<String, String> {
    match path {
        Some(p) => std::fs::read_to_string(p).map_err(|e| e.to_string()),
        None => {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .map_err(|e| e.to_string())?;
            Ok(s)
        }
    }
}

struct Opts {
    receipt_path: Option<PathBuf>,
    input_path: Option<PathBuf>,
    output_path: Option<PathBuf>,
}

const USAGE: &str = "\
Usage: maranode-verify [OPTIONS] [RECEIPT_FILE]

Verifies a Maranode inference receipt offline.
If RECEIPT_FILE is omitted, reads JSON from stdin.

Options:
  --input  FILE   re-check input hash against FILE
  --output FILE   re-check output hash against FILE
  --help          print this message
";

fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut receipt_path = None;
    let mut input_path = None;
    let mut output_path = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--input" => {
                i += 1;
                input_path = Some(PathBuf::from(
                    args.get(i).ok_or("--input requires a file argument")?,
                ));
            }
            "--output" => {
                i += 1;
                output_path = Some(PathBuf::from(
                    args.get(i).ok_or("--output requires a file argument")?,
                ));
            }
            s if s.starts_with('-') => return Err(format!("unknown flag: {s}")),
            path => {
                receipt_path = Some(PathBuf::from(path));
            }
        }
        i += 1;
    }

    Ok(Opts { receipt_path, input_path, output_path })
}
