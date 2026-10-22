//! Malformed model files must return an error through both public native loaders.
//! Each case runs in a bounded child process so a native abort cannot kill the suite.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use whisper_rs::{WhisperContext, WhisperContextParameters};

const CASE_PATH: &str = "SOTTO_WHISPER_PARSER_CASE";

fn ints(values: &[i32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn header() -> Vec<u8> {
    // Original Whisper tiny architecture, with an empty serialized vocabulary
    // (the native format supplies the missing special tokens itself).
    let mut data = ints(&[
        0x6767_6d6c,
        51864,
        1500,
        384,
        6,
        4,
        448,
        384,
        6,
        4,
        80,
        1,
        80,
        201,
    ]);
    data.resize(data.len() + 80 * 201 * 4, 0);
    data.extend(0_i32.to_le_bytes());
    data
}

fn scalar(data: &mut [u8], index: usize, value: i32) {
    data[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
}

fn tensor(dims: &[i32], kind: i32, name: &[u8], payload: usize) -> Vec<u8> {
    let mut data = ints(&[dims.len() as i32, name.len() as i32, kind]);
    data.extend(ints(dims));
    data.extend(name);
    data.resize(data.len() + payload, 0);
    data
}

fn cases() -> Vec<(String, Vec<u8>)> {
    let valid = header();
    let mut cases = Vec::new();
    for length in [0, 1, 3, 4, 7, 47, 48, 51, 55, valid.len() - 1] {
        cases.push((format!("truncated-{length}"), valid[..length].to_vec()));
    }
    cases.push(("empty-weights".into(), valid.clone()));
    for (index, value, name) in [
        (1, 0, "vocab-zero"),
        (1, 51863, "vocab-special-tokens-missing"),
        (1, 51867, "vocab-unsupported-language"),
        (2, 0, "audio-context-zero"),
        (2, i32::MAX, "audio-context-overflow"),
        (3, 0, "state-zero"),
        (3, i32::MAX, "state-overflow"),
        (4, 0, "audio-head-zero"),
        (4, 5, "audio-head-incompatible"),
        (5, -1, "audio-layers-negative"),
        (5, i32::MAX, "audio-layers-overflow"),
        (6, 0, "text-context-zero"),
        (6, 9, "text-context-prompt-overflow"),
        (6, i32::MAX, "text-context-overflow"),
        (7, 385, "state-mismatch"),
        (8, 0, "text-head-zero"),
        (8, 5, "text-head-incompatible"),
        (9, i32::MAX, "text-layers-overflow"),
        (10, 0, "mel-zero"),
        (10, 65537, "mel-overflow"),
        (11, -1, "ftype-negative"),
        (11, 4, "ftype-unsupported"),
        (11, 999, "ftype-unknown"),
        (12, -1, "filter-negative"),
        (12, 128, "filter-mismatch"),
        (12, 65537, "filter-multiply-overflow"),
        (13, 200, "filter-fft-small"),
        (13, 202, "filter-fft-large"),
        (13, 65536, "filter-fft-overflow"),
    ] {
        let mut data = valid.clone();
        scalar(&mut data, index, value);
        cases.push((name.into(), data));
    }
    for count in [-1_i32, 51865] {
        let mut data = valid.clone();
        let offset = data.len() - 4;
        data[offset..].copy_from_slice(&count.to_le_bytes());
        cases.push((format!("serialized-vocab-{count}"), data));
    }
    let vocab_offset = valid.len() - 4;
    let mut unaligned_quantization = valid.clone();
    for (index, value) in [(3, 385), (4, 5), (7, 385), (8, 5), (11, 2)] {
        scalar(&mut unaligned_quantization, index, value);
    }
    cases.push(("quantization-row-alignment".into(), unaligned_quantization));
    for length in [1_u32, u32::MAX] {
        let mut data = valid[..vocab_offset].to_vec();
        data.extend(1_i32.to_le_bytes());
        data.extend(length.to_le_bytes());
        cases.push((format!("truncated-vocab-token-{length}"), data));
    }
    let mut oversized_vocab = valid[..vocab_offset].to_vec();
    oversized_vocab.extend(17_i32.to_le_bytes());
    for _ in 0..17 {
        oversized_vocab.extend((1024_u32 * 1024).to_le_bytes());
        oversized_vocab.resize(oversized_vocab.len() + 1024 * 1024, b'a');
    }
    cases.push(("vocab-total-budget".into(), oversized_vocab));

    let bias = b"encoder.ln_post.bias";
    for dims in [-1, 0, 5, i32::MAX] {
        let mut data = valid.clone();
        data.extend(ints(&[dims, bias.len() as i32, 0]));
        cases.push((format!("tensor-dimensions-{dims}"), data));
    }
    for kind in [-1, 1, 4, 5, 26, 31, 38, 39, i32::MAX] {
        let mut data = valid.clone();
        data.extend(tensor(&[384], kind, bias, 384 * 4));
        cases.push((format!("tensor-type-{kind}"), data));
    }
    for length in [-1, 0, i32::MAX] {
        let mut data = valid.clone();
        data.extend(ints(&[1, length, 0, 384]));
        cases.push((format!("tensor-name-length-{length}"), data));
    }
    for (name, dims) in [
        ("zero", vec![0]),
        ("negative", vec![-384]),
        ("overflow", vec![i32::MAX, i32::MAX]),
        ("fourth-dimension", vec![384, 1, 1, 2]),
    ] {
        let mut data = valid.clone();
        data.extend(tensor(&dims, 0, bias, 384 * 4));
        cases.push((format!("tensor-shape-{name}"), data));
    }
    for (name, bytes) in [
        ("unknown", b"unknown.tensor".as_slice()),
        ("nul", b"encoder.ln_post.bias\0".as_slice()),
    ] {
        let mut data = valid.clone();
        data.extend(tensor(&[384], 0, bytes, 384 * 4));
        cases.push((format!("tensor-name-{name}"), data));
    }
    let record = tensor(&[384], 0, bias, 384 * 4);
    for length in [1, 4, 8, 12, 15, 16, 20, record.len() - 1, record.len()] {
        let mut data = valid.clone();
        data.extend(&record[..length]);
        cases.push((format!("incomplete-tensors-{length}"), data));
    }
    let mut duplicate = valid.clone();
    duplicate.extend(&record);
    duplicate.extend(&record);
    cases.push(("duplicate-tensor".into(), duplicate));
    let mut balanced_duplicates = valid;
    // Tiny has 11 global tensors, 15 per encoder layer and 24 per decoder
    // layer. A record count alone must not let duplicates replace missing weights.
    for _ in 0..11 + 15 * 4 + 24 * 4 {
        balanced_duplicates.extend(&record);
    }
    cases.push((
        "duplicates-replacing-missing-weights".into(),
        balanced_duplicates,
    ));
    cases
}

#[test]
fn malformed_models_return_errors_without_crashing_or_hanging() {
    let directory = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();
    for (name, bytes) in cases() {
        let path = directory.path().join(format!("{name}.bin"));
        fs::write(&path, bytes).unwrap();
        let log_path = directory.path().join(format!("{name}.log"));
        let log = fs::File::create(&log_path).unwrap();
        let mut child = Command::new(&executable)
            .args([
                "--exact",
                "malformed_model_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CASE_PATH, &path)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if started.elapsed() > Duration::from_secs(30) {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("native model loader timed out: {name}");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().unwrap();
        if !status.success() {
            panic!(
                "malformed model {name}: {status}\n{}",
                fs::read_to_string(log_path).unwrap()
            );
        }
    }
}

#[test]
#[ignore = "Invoked with an isolated synthetic fixture by the parent regression test"]
fn malformed_model_child() {
    let path = PathBuf::from(std::env::var_os(CASE_PATH).expect("missing isolated parser fixture"));
    let parameters = || {
        let mut parameters = WhisperContextParameters::default();
        parameters.use_gpu(false);
        parameters
    };
    assert!(WhisperContext::new_with_params(path.to_str().unwrap(), parameters()).is_err());
    let bytes = fs::read(path).unwrap();
    assert!(WhisperContext::new_from_buffer_with_params(&bytes, parameters()).is_err());
}
