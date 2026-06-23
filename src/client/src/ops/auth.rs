use crate::auth::{PairStartResponse, PairStatusResponse};
use crate::operation::Operation;

pub fn pair_start() -> Operation<PairStartResponse> {
    Operation::new(crate::auth::pair_start(), crate::auth::decode_pair_start)
}

pub fn pair_status(sid: &str) -> Operation<PairStatusResponse> {
    Operation::new(
        crate::auth::pair_status(sid),
        crate::auth::decode_pair_status,
    )
}
