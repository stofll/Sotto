//! Pinned public 16 kHz mono PCM fixtures, fetched into memory without user data.

pub async fn speech_sample(client: &reqwest::Client, url: &str, sha256: &str) -> Vec<f32> {
    use sha2::{Digest, Sha256};
    let bytes = client
        .get(url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        sha256,
        "fixture checksum"
    );
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(
        u16::from_le_bytes(bytes[20..22].try_into().unwrap()),
        1,
        "PCM"
    );
    assert_eq!(
        u16::from_le_bytes(bytes[22..24].try_into().unwrap()),
        1,
        "mono"
    );
    assert_eq!(
        u16::from_le_bytes(bytes[34..36].try_into().unwrap()),
        16,
        "16 bit"
    );
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        16_000,
        "the sample is expected to be 16 kHz"
    );
    let mut at = 12;
    loop {
        let chunk_id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if chunk_id == b"data" {
            return bytes[at + 8..at + 8 + size]
                .chunks_exact(2)
                .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
                .collect();
        }
        at += 8 + size + (size & 1);
    }
}
