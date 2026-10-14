use crate::error::DemoError;
use frostline_shared::codec::drop_schemas;
use frostline_shared::output::act;
use frostline_shared::runfile::RunFile;
use frostline_shared::topology;
use laser_sdk::filters::FilterErrorReason;
use laser_sdk::prelude::{Laser, LaserError};
use laser_sdk::wire::codes::{
    AGDX_FILTER_MUTATE_CODE, AGDX_FILTER_OPERATION_CODE, FILTER_OP_VERSION,
};
use laser_sdk::wire::filter::{
    FilterCatalogOutcome, FilterCatalogReply, FilterMutation, FilterMutationRequest,
    FilterMutationResult, FilterMutationStatus, GetFilterOperation,
};
use laser_sdk::wire::framing::{decode_named, encode_named};
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Release the run's exact bindings and retire their owned definitions before deleting the stream.
pub async fn cleanup(laser: &Laser, run_file: &RunFile) -> Result<(), DemoError> {
    for record in &run_file.groups {
        if let Some(binding) = &record.binding {
            retire_binding(laser, binding).await?;
        }
    }
    drop_schemas(
        &laser.with_default_stream(run_file.run_id.stream()),
        &run_file.schema_ids,
    )
    .await?;
    let removed = topology::delete_run(laser, &run_file.run_id).await?;
    act(&match removed {
        true => format!("Deleted stream {}.", run_file.run_id.stream()),
        false => format!("Stream {} was already gone.", run_file.run_id.stream()),
    });
    Ok(())
}

/// Retire a temporary group's exact policy. Repeating cleanup returns its original outcomes.
pub async fn retire_binding(
    laser: &Laser,
    binding: &laser_sdk::filters::FilterBinding,
) -> Result<(), LaserError> {
    let released = mutate(
        laser,
        FilterMutation::Unbind {
            group: binding.group.clone(),
            expected_digest: binding.digest.clone(),
            expected_identity: Some(binding.identity),
        },
    )
    .await;
    match released {
        Ok(_) => {}
        Err(error) if error.filter_reason() == Some(FilterErrorReason::NotFound) => {}
        Err(error) => return Err(error),
    }
    mutate(
        laser,
        FilterMutation::Drop {
            filter_id: binding.filter_id,
        },
    )
    .await?;
    Ok(())
}

// Administrative cleanup uses the existing wire verbs. Stable operation IDs
// make retries return the first outcome even after a successful prior cleanup.
async fn mutate(
    laser: &Laser,
    mutation: FilterMutation,
) -> Result<FilterMutationResult, LaserError> {
    let mutation_bytes =
        encode_named(&mutation).map_err(|error| LaserError::Codec(error.to_string()))?;
    let hash = Sha256::digest([b"frostline group cleanup v1".as_slice(), &mutation_bytes].concat());
    let operation_id = u128::from_le_bytes(hash[..16].try_into().expect("digest prefix")).max(1);
    let request = FilterMutationRequest {
        v: FILTER_OP_VERSION,
        operation_id,
        mutation,
    };
    let body = encode_named(&request).map_err(|error| LaserError::Codec(error.to_string()))?;
    let mut reply = laser
        .client()
        .send_binary_request(AGDX_FILTER_MUTATE_CODE, body.into())
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let outcome: FilterCatalogReply =
            decode_named(&reply).map_err(|error| LaserError::Codec(error.to_string()))?;
        let status = match outcome {
            FilterCatalogReply::Ok(outcome) => match *outcome {
                FilterCatalogOutcome::Mutation(outcome) if outcome.operation_id == operation_id => {
                    outcome.status
                }
                _ => return Err(LaserError::Invalid("unexpected cleanup outcome".to_owned())),
            },
            FilterCatalogReply::Err(error) => return Err(error.into()),
        };
        match status {
            FilterMutationStatus::Applied(result) => return Ok(result),
            FilterMutationStatus::Rejected(error) => return Err(error.into()),
            FilterMutationStatus::Pending => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(LaserError::Timeout("group cleanup outcome"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        let body = encode_named(&GetFilterOperation {
            v: FILTER_OP_VERSION,
            operation_id,
        })
        .map_err(|error| LaserError::Codec(error.to_string()))?;
        reply = laser
            .client()
            .send_binary_request(AGDX_FILTER_OPERATION_CODE, body.into())
            .await?;
    }
}
