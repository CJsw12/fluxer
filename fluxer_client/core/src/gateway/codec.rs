//! zstd-stream codec for the gateway (`compress=zstd-stream&stream=1`).
//!
//! One context per direction lives for the whole connection; every WebSocket
//! binary message is one flushed chunk of that single stream (no dictionary).

use anyhow::{Context, Result, bail};
use zstd::stream::raw::{CParameter, DParameter, Decoder, Encoder, InBuffer, Operation, OutBuffer};

/// Client encoder parameters (`zstd_stream.rs:8-10`, `GatewayCompression.ts:19`).
const ENCODER_LEVEL: i32 = 3;
const ENCODER_WINDOW_LOG: u32 = 17;
const ENCODER_HASH_LOG: u32 = 15;
const ENCODER_CHAIN_LOG: u32 = 15;
/// The server may raise its window log up to 21 (`gateway_compress.erl`).
pub(crate) const DECODER_WINDOW_LOG_MAX: u32 = 21;
/// Upper bound for one decompressed message (READY can be large).
pub(crate) const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024 * 1024;
const SCRATCH_BYTES: usize = 16 * 1024;

/// Server -> client stream decoder.
pub(crate) struct ZstdInflater {
    decoder: Decoder<'static>,
    scratch: Vec<u8>,
}

impl ZstdInflater {
    pub(crate) fn new() -> Result<Self> {
        let mut decoder = Decoder::new().context("create zstd decoder")?;
        decoder
            .set_parameter(DParameter::WindowLogMax(DECODER_WINDOW_LOG_MAX))
            .context("set zstd windowLogMax")?;
        Ok(Self {
            decoder,
            scratch: vec![0; SCRATCH_BYTES],
        })
    }

    /// Feeds one binary message. Returns `None` when the input produced no
    /// output yet (wait for the next chunk, as `GatewaySocket.ts:897-903`).
    pub(crate) fn decompress(&mut self, input: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut src = InBuffer::around(input);
        let mut out = Vec::new();
        loop {
            let mut dst = OutBuffer::around(&mut self.scratch[..]);
            self.decoder
                .run(&mut src, &mut dst)
                .context("zstd decompress")?;
            let written = dst.pos();
            out.extend_from_slice(&self.scratch[..written]);
            if out.len() > MAX_DECOMPRESSED_BYTES {
                bail!("decompressed gateway message exceeds {MAX_DECOMPRESSED_BYTES} bytes");
            }
            // Input consumed and the output buffer was not filled: nothing pending.
            if src.pos() == input.len() && written < self.scratch.len() {
                break;
            }
        }
        Ok((!out.is_empty()).then_some(out))
    }
}

/// Client -> server stream encoder; flushes after every message.
pub(crate) struct ZstdDeflater {
    encoder: Encoder<'static>,
    scratch: Vec<u8>,
}

impl ZstdDeflater {
    pub(crate) fn new() -> Result<Self> {
        Self::with_window_log(ENCODER_WINDOW_LOG)
    }

    pub(crate) fn with_window_log(window_log: u32) -> Result<Self> {
        let mut encoder = Encoder::new(ENCODER_LEVEL).context("create zstd encoder")?;
        for parameter in [
            CParameter::WindowLog(window_log),
            CParameter::HashLog(ENCODER_HASH_LOG),
            CParameter::ChainLog(ENCODER_CHAIN_LOG),
        ] {
            encoder
                .set_parameter(parameter)
                .context("set zstd encoder parameter")?;
        }
        Ok(Self {
            encoder,
            scratch: vec![0; SCRATCH_BYTES],
        })
    }

    pub(crate) fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        let mut src = InBuffer::around(input);
        let mut out = Vec::new();
        while src.pos() < input.len() {
            let mut dst = OutBuffer::around(&mut self.scratch[..]);
            self.encoder
                .run(&mut src, &mut dst)
                .context("zstd compress")?;
            let written = dst.pos();
            out.extend_from_slice(&self.scratch[..written]);
        }
        loop {
            let mut dst = OutBuffer::around(&mut self.scratch[..]);
            let remaining = self.encoder.flush(&mut dst).context("zstd flush")?;
            let written = dst.pos();
            out.extend_from_slice(&self.scratch[..written]);
            if remaining == 0 {
                break;
            }
        }
        Ok(out)
    }
}
