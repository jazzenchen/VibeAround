use crate::operation::Operation;
use crate::service::{ServiceHealthResponse, ServiceInfoResponse};

pub fn service_health() -> Operation<ServiceHealthResponse> {
    Operation::new(crate::service::health(), crate::service::decode_health)
}

pub fn service_info() -> Operation<ServiceInfoResponse> {
    Operation::new(crate::service::info(), crate::service::decode_info)
}
