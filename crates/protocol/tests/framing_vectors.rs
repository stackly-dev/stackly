//! Runs every vector in `protocols/fixtures/framing/vectors.json` through
//! the frame codec (spec 0006 AC-6). Feature 13 runs the same vectors
//! against the Swift transport.

use std::path::PathBuf;

use serde::Deserialize;
use stackly_protocol::frame::{FrameDecoder, encode_frame};

#[derive(Debug, Deserialize)]
struct Vectors {
    vectors: Vec<Vector>,
}

#[derive(Debug, Deserialize)]
struct Vector {
    name: String,
    hex: String,
    #[serde(default)]
    chunks: Option<Vec<usize>>,
    max: u32,
    #[serde(default)]
    frames: Vec<String>,
    #[serde(default)]
    error: Option<String>,
}

/// What a decoder produced for one stream: the frames, then the error that
/// stopped it, if any.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    frames: Vec<Vec<u8>>,
    error: Option<String>,
}

fn vectors() -> Vec<Vector> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/framing/vectors.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    let vectors: Vectors = serde_json::from_str(&text).expect("vectors.json matches its shape");
    vectors.vectors
}

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "odd hex length in `{text}`");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("vectors hold only hex digits"))
        .collect()
}

/// Feeds `chunks` in order, draining frames after each push as a transport
/// does, and stops at the first error.
fn decode<'a>(max: u32, chunks: impl IntoIterator<Item = &'a [u8]>) -> Outcome {
    let mut decoder = FrameDecoder::new(max);
    let mut frames = Vec::new();
    for chunk in chunks {
        let result = decoder.push(chunk);
        frames.extend(std::iter::from_fn(|| decoder.next_frame()));
        if let Err(err) = result {
            return Outcome {
                frames,
                error: Some(err.to_error_body().code().to_owned()),
            };
        }
    }
    Outcome {
        frames,
        error: None,
    }
}

/// Cuts `bytes` by the listed chunk sizes; what remains is one last chunk.
fn cut<'a>(bytes: &'a [u8], sizes: &[usize]) -> Vec<&'a [u8]> {
    let mut chunks = Vec::new();
    let mut rest = bytes;
    for &size in sizes {
        let (head, tail) = rest.split_at(size.min(rest.len()));
        chunks.push(head);
        rest = tail;
    }
    if !rest.is_empty() {
        chunks.push(rest);
    }
    chunks
}

fn expected(vector: &Vector) -> Outcome {
    Outcome {
        frames: vector.frames.iter().map(|hex| unhex(hex)).collect(),
        error: vector.error.clone(),
    }
}

#[test]
fn every_vector_decodes_as_listed_with_its_chunks() {
    let vectors = vectors();
    assert!(vectors.len() >= 8);
    for vector in &vectors {
        let bytes = unhex(&vector.hex);
        let sizes = vector.chunks.clone().unwrap_or_default();
        assert_eq!(
            decode(vector.max, cut(&bytes, &sizes)),
            expected(vector),
            "{}",
            vector.name
        );
    }
}

#[test]
fn every_vector_decodes_the_same_for_every_two_way_split_and_byte_by_byte() {
    for vector in vectors() {
        let bytes = unhex(&vector.hex);
        let want = expected(&vector);
        for split in 0..=bytes.len() {
            let (head, tail) = bytes.split_at(split);
            assert_eq!(
                decode(vector.max, [head, tail]),
                want,
                "{} split at {split}",
                vector.name
            );
        }
        let per_byte = bytes.chunks(1);
        assert_eq!(
            decode(vector.max, per_byte),
            want,
            "{} byte by byte",
            vector.name
        );
    }
}

#[test]
fn encoding_the_listed_frames_gives_back_each_complete_vector() {
    for vector in vectors() {
        if vector.error.is_some() || vector.frames.is_empty() {
            continue;
        }
        let mut stream = Vec::new();
        for frame in &vector.frames {
            stream.extend(encode_frame(&unhex(frame), vector.max).unwrap());
        }
        assert_eq!(stream, unhex(&vector.hex), "{}", vector.name);
    }
}

#[test]
fn the_vectors_cover_the_cases_spec_0006_lists() {
    let names: Vec<String> = vectors().into_iter().map(|v| v.name).collect();
    for name in [
        "two frames in one chunk",
        "one frame cut at every byte",
        "length of zero",
        "length of exactly max",
        "length of max plus one",
        "truncated header",
    ] {
        assert!(names.iter().any(|n| n == name), "no vector `{name}`");
    }
}
