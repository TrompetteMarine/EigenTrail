//! Domain-separated wire integrity. Digests do not establish numerical correctness or coverage.
use super::{
    SCHEMA,
    report::{CandidateBatch, CandidateStream, Chunk, Verdict},
};
use crate::{
    Error, Result,
    resources::{Cancellation, memory},
    serialization,
};
use sha2::{Digest, Sha256};
#[derive(Clone)]
pub struct CandidateHasher(Sha256);
fn number(hash: &mut Sha256, value: usize) -> Result<()> {
    hash.update(
        u64::try_from(value)
            .map_err(|_| Error::Resource(String::new()))?
            .to_le_bytes(),
    );
    Ok(())
}
impl CandidateHasher {
    pub fn new(
        request_hash: &str,
        stream: CandidateStream,
        window: Option<[usize; 3]>,
    ) -> Result<Self> {
        let mut hash = Sha256::new();
        hash.update(b"EigenTrail.rolling.candidates.v1\0");
        hash.update(SCHEMA.as_bytes());
        hash.update([0]);
        hash.update(request_hash.as_bytes());
        hash.update([match stream {
            CandidateStream::Graph => 0,
            CandidateStream::Direct => 1,
        }]);
        match window {
            Some(values) => {
                hash.update([1]);
                for value in values {
                    number(&mut hash, value)?;
                }
            }
            None => hash.update([0]),
        }
        Ok(Self(hash))
    }
    pub fn push(
        &mut self,
        ordinal: usize,
        source: usize,
        target: usize,
        verdict: Verdict,
    ) -> Result<()> {
        number(&mut self.0, ordinal)?;
        number(&mut self.0, source)?;
        number(&mut self.0, target)?;
        self.0.update([match verdict {
            Verdict::Agreement => 0,
            Verdict::Exclusion => 1,
            Verdict::Open => 2,
            Verdict::Unavailable => 3,
        }]);
        Ok(())
    }
    pub fn finish(self) -> Result<String> {
        memory::format(format_args!("{:x}", self.0.finalize()))
    }
}
pub fn chunk_seed(request_hash: &str, cancel: &Cancellation) -> Result<String> {
    serialization::digest_with_cancel(
        &("EigenTrail.rolling.chunks.v1", SCHEMA, request_hash),
        cancel,
    )
}
pub fn chunk_digest(request_hash: &str, chunk: &Chunk, cancel: &Cancellation) -> Result<String> {
    serialization::digest_with_cancel(
        &(
            "EigenTrail.rolling.chunk.v1",
            SCHEMA,
            request_hash,
            chunk.index,
            chunk.first_record,
            &chunk.prev_digest,
            &chunk.records,
        ),
        cancel,
    )
}
pub fn candidate_batch_digest(
    request_hash: &str,
    batch: &CandidateBatch,
    cancel: &Cancellation,
) -> Result<String> {
    serialization::digest_with_cancel(
        &(
            "EigenTrail.rolling.batch.v1",
            SCHEMA,
            request_hash,
            batch.stream,
            batch.first,
            &batch.decisions,
        ),
        cancel,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidate_order_identity_scope_and_verdict_are_bound() {
        let baseline = || CandidateHasher::new("request", CandidateStream::Graph, None).unwrap();
        let mut a = baseline();
        a.push(0, 1, 2, Verdict::Agreement).unwrap();
        let expected = a.finish().unwrap();
        for (ordinal, source, target, verdict) in [
            (1, 1, 2, Verdict::Agreement),
            (0, 2, 1, Verdict::Agreement),
            (0, 1, 2, Verdict::Exclusion),
        ] {
            let mut a = baseline();
            a.push(ordinal, source, target, verdict).unwrap();
            assert_ne!(a.finish().unwrap(), expected);
        }
        for (request, stream, window) in [
            ("changed", CandidateStream::Graph, None),
            ("request", CandidateStream::Direct, None),
            ("request", CandidateStream::Graph, Some([0, 1, 2])),
        ] {
            let mut a = CandidateHasher::new(request, stream, window).unwrap();
            a.push(0, 1, 2, Verdict::Agreement).unwrap();
            assert_ne!(a.finish().unwrap(), expected);
        }
    }
}
