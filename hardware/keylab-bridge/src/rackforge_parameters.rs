//! Queue coalescing for RackForge-owned semantic parameters.
//!
//! The physical-to-semantic mapping itself belongs to the `.rfcontroller`
//! profile and canonical value conversion belongs to `rackforge-session-api`.

use rackforge_session_api::{RackForgeParameterId, RackForgeParameterInput};

pub(crate) fn coalesce_rackforge_parameters(
    events: impl IntoIterator<Item = RackForgeParameterInput>,
) -> Vec<RackForgeParameterInput> {
    let mut latest_level = None;
    let mut latest_pan = None;
    let mut last_parameter = None;
    for event in events {
        match event.parameter {
            RackForgeParameterId::MasterLevel => latest_level = Some(event),
            RackForgeParameterId::MasterPan => latest_pan = Some(event),
        }
        last_parameter = Some(event.parameter);
    }

    let mut coalesced = Vec::with_capacity(2);
    match last_parameter {
        Some(RackForgeParameterId::MasterLevel) => {
            coalesced.extend(latest_pan);
            coalesced.extend(latest_level);
        }
        Some(RackForgeParameterId::MasterPan) => {
            coalesced.extend(latest_level);
            coalesced.extend(latest_pan);
        }
        None => {}
    }
    coalesced
}
