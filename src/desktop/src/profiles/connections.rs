//! Desktop-facing wrappers for shared profile-to-agent connection routing.

pub(super) use common::profiles::connections::{
    merged_profile_connections, profile_can_launch_agent, resolve_profile_agent_route,
};
