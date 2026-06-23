use crate::operation::Operation;
use crate::previews::PreviewsResponse;

use super::decode_success;

pub fn previews() -> Operation<PreviewsResponse> {
    Operation::new(crate::previews::list(), crate::previews::decode_list)
}

pub fn preview_delete(slug: &str) -> Operation<()> {
    Operation::new(crate::previews::delete(slug), decode_success)
}
