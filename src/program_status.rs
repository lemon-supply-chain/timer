//! Writer for the OSC 7501 Program Status Protocol.
//!
//! A program reports what it is doing as a list of `key=value` pairs inside an
//! OSC sequence:
//!
//! ```text
//! ESC ] 7501 ; state=working:app=timer:progress=42 ESC \
//! ```
//!
//! This module implements the subset the countdown needs: the `idle`,
//! `working`, `done` and `error` states on the root record, plus the optional
//! `progress` and `msg` keys. Reports are best effort; terminals that do not
//! know OSC 7501 ignore them.

use crate::Result;

use std::io::Write;

/// Stable identifier reported through the `app` key.
pub const APP_ID: &str = "timer";

/// Introducer of an OSC 7501 report.
const OSC: &str = "\x1b]7501;";
/// String terminator of an OSC sequence (`ESC \`).
const ST: &str = "\x1b\\";
/// Hard cap for a whole sequence, from OSC through ST.
#[cfg(any(debug_assertions, test))]
const MAX_SEQUENCE: usize = 4096;
/// Hard cap for the encoded `msg` value.
const MAX_MSG_ENCODED: usize = 2732;
/// Hard cap for the decoded `msg` value.
const MAX_MSG_DECODED: usize = 2048;

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// State of the program, reported through the required `state` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// At rest, waiting for the user.
    Idle,
    /// Running, may carry `progress`.
    Working,
    /// Finished a piece of work the user has not looked at yet.
    Done,
    /// Failed and stopped.
    Error,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Working => "working",
            State::Done => "done",
            State::Error => "error",
        }
    }
}

/// A single OSC 7501 report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    state: Option<State>,
    progress: Option<u8>,
    msg: Option<String>,
}

impl Report {
    pub fn new(state: State) -> Self {
        Self {
            state: Some(state),
            ..Self::default()
        }
    }

    /// Sets the completion percentage, only reported while `working`.
    ///
    /// Values above 100 are clamped; an absent progress means indeterminate.
    pub fn progress(mut self, progress: u8) -> Self {
        self.progress = Some(progress.min(100));
        self
    }

    /// Sets the single line reason shown by the terminal.
    ///
    /// Messages holding control characters or exceeding the protocol limits are
    /// dropped, which keeps the state part of the report valid.
    pub fn msg(mut self, msg: impl Into<String>) -> Self {
        self.msg = Some(msg.into());
        self
    }

    pub fn to_sequence(&self) -> String {
        let state = self.state.unwrap_or(State::Idle);

        let mut seq = String::from(OSC);
        seq.push_str("state=");
        seq.push_str(state.as_str());
        seq.push_str(":app=");
        seq.push_str(APP_ID);

        if let Some(progress) = self.progress
            && state == State::Working
        {
            seq.push_str(":progress=");
            seq.push_str(&progress.to_string());
        }

        if let Some(encoded) = self
            .msg
            .as_deref()
            .and_then(|msg| encode_free_text(msg, MAX_MSG_ENCODED, MAX_MSG_DECODED))
        {
            seq.push_str(":msg=");
            seq.push_str(&encoded);
        }

        seq.push_str(ST);
        #[cfg(debug_assertions)]
        debug_assert!(seq.len() <= MAX_SEQUENCE, "OSC 7501 report is too long");
        seq
    }
}

/// Writes a report to `w`.
pub fn write_report<W>(w: &mut W, report: &Report) -> Result<()>
where
    W: Write,
{
    w.write_all(report.to_sequence().as_bytes())?;
    w.flush()?;
    Ok(())
}

/// Encodes `text` as a protocol value, or returns `None` when it is not usable.
fn encode_free_text(text: &str, max_encoded: usize, max_decoded: usize) -> Option<String> {
    if text.len() > max_decoded || text.chars().any(char::is_control) {
        return None;
    }

    let encoded = base64_encode(text.as_bytes());
    (encoded.len() <= max_encoded).then_some(encoded)
}

/// Standard base64 of `input`, with padding.
pub fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);

    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(char::from(
            BASE64_ALPHABET[((triple >> 18) & 0x3f) as usize],
        ));
        out.push(char::from(
            BASE64_ALPHABET[((triple >> 12) & 0x3f) as usize],
        ));
        out.push(match chunk.len() > 1 {
            true => char::from(BASE64_ALPHABET[((triple >> 6) & 0x3f) as usize]),
            false => '=',
        });
        out.push(match chunk.len() > 2 {
            true => char::from(BASE64_ALPHABET[(triple & 0x3f) as usize]),
            false => '=',
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_padding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_base64_encode_utf8() {
        assert_eq!(base64_encode("时间到".as_bytes()), "5pe26Ze05Yiw");
    }

    #[test]
    fn test_sequence_working() {
        let report = Report::new(State::Working);
        assert_eq!(
            report.to_sequence(),
            "\x1b]7501;state=working:app=timer\x1b\\"
        );
    }

    #[test]
    fn test_sequence_working_with_progress() {
        let report = Report::new(State::Working).progress(42);
        assert_eq!(
            report.to_sequence(),
            "\x1b]7501;state=working:app=timer:progress=42\x1b\\"
        );
    }

    #[test]
    fn test_sequence_clamps_progress() {
        let report = Report::new(State::Working).progress(200);
        assert_eq!(
            report.to_sequence(),
            "\x1b]7501;state=working:app=timer:progress=100\x1b\\"
        );
    }

    #[test]
    fn test_sequence_drops_progress_when_not_working() {
        let report = Report::new(State::Done).progress(50);
        assert_eq!(report.to_sequence(), "\x1b]7501;state=done:app=timer\x1b\\");
    }

    #[test]
    fn test_sequence_with_msg() {
        let report = Report::new(State::Done).msg("Time is up");
        assert_eq!(
            report.to_sequence(),
            "\x1b]7501;state=done:app=timer:msg=VGltZSBpcyB1cA==\x1b\\"
        );
    }

    #[test]
    fn test_sequence_drops_msg_with_control_characters() {
        let report = Report::new(State::Done).msg("Time\tis up");
        assert_eq!(report.to_sequence(), "\x1b]7501;state=done:app=timer\x1b\\");
    }

    #[test]
    fn test_sequence_drops_oversized_msg() {
        let report = Report::new(State::Done).msg("a".repeat(MAX_MSG_DECODED + 1));
        assert_eq!(report.to_sequence(), "\x1b]7501;state=done:app=timer\x1b\\");
    }

    #[test]
    fn test_sequence_ids_and_states() {
        for (state, expected) in [
            (State::Idle, "idle"),
            (State::Working, "working"),
            (State::Done, "done"),
            (State::Error, "error"),
        ] {
            let seq = Report::new(state).to_sequence();
            assert!(seq.starts_with(&format!("\x1b]7501;state={expected}:app=timer")));
            assert!(seq.ends_with("\x1b\\"));
        }
    }

    #[test]
    fn test_write_report() {
        let mut buf = Vec::new();
        write_report(&mut buf, &Report::new(State::Idle)).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "\x1b]7501;state=idle:app=timer\x1b\\"
        );
    }

    #[test]
    fn test_sequence_length_within_limit() {
        let report = Report::new(State::Working)
            .progress(99)
            .msg("a".repeat(MAX_MSG_DECODED));
        assert!(report.to_sequence().len() <= MAX_SEQUENCE);
    }
}
