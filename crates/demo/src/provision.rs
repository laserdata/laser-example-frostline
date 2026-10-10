use crate::error::DemoError;
use frostline_producer::fleet::GENERATOR_VERSION;
use frostline_shared::Settings;
use frostline_shared::codec::{Codec, SchemaSet, filter_headers, register_schemas};
use frostline_shared::domain::{Cargo, Region};
use frostline_shared::event::FleetEvent;
use frostline_shared::fixtures;
use frostline_shared::measure::hex;
use frostline_shared::names::{Header, Role, RunId, RunTopic, SAFETY_CURRENT_GROUP};
use frostline_shared::output::act;
use frostline_shared::policy::{self, RolePolicy};
use frostline_shared::runfile::{GroupRecord, RunFile};
use frostline_shared::topology::{self, Retention};
use laser_sdk::filters::{FilterHeader, HeaderScalar, Verdict};
use laser_sdk::prelude::{ConsumerGroup, Laser};
use std::path::PathBuf;

const RUN_FILE: &str = "run.json";

/// A run ready to read: its topics, writer schema, saved filters, bound groups, and run file.
pub struct Provisioned {
    pub run_file: RunFile,
    pub path: PathBuf,
    pub directory: PathBuf,
    pub schemas: SchemaSet,
}

pub async fn provision(
    laser: &Laser,
    settings: &Settings,
    host: String,
) -> Result<Provisioned, DemoError> {
    let run_id = RunId::mint();
    // Writer schemas and the filters that reference them are scoped to the
    // client's default stream, so provisioning runs on the run's own stream.
    let laser = &laser.with_default_stream(run_id.stream());
    let directory = settings
        .output_directory
        .clone()
        .unwrap_or_else(|| PathBuf::from("runs").join(run_id.as_str()));
    let retention = Retention {
        changes: settings.changes_expiry,
        reports: settings.reports_expiry,
    };
    let source = topology::ensure_run(laser, &run_id, settings.partitions, retention).await?;

    act(&format!(
        "Created stream {} with {} partitions of changes and one of reports, retention verified.",
        run_id.stream(),
        settings.partitions
    ));
    let mut provisioned = Provisioned {
        run_file: RunFile {
            run_id: run_id.clone(),
            settings_digest: settings.digest(),
            generator_version: GENERATOR_VERSION.to_owned(),
            host,
            partitions: settings.partitions,
            workers_per_role: settings.workers_per_role,
            checkpoint_records: settings.checkpoint_records,
            codec: settings.codec,
            catalog: settings.catalog,
            source,
            schema_ids: Vec::new(),
            groups: Vec::new(),
        },
        path: directory.join(RUN_FILE),
        directory,
        schemas: SchemaSet::default(),
    };
    provisioned.run_file.write(&provisioned.path)?;
    let outcome = async {
        topology::verify_retention(laser, &run_id, retention).await?;
        provisioned.schemas = register_schemas(laser, settings.codec).await?;
        for id in provisioned.schemas.ids() {
            act(&format!(
                "Registered writer schema {id} for {} payloads.",
                settings.codec
            ));
        }
        provisioned.run_file.schema_ids = provisioned.schemas.ids();
        provisioned.run_file.write(&provisioned.path)?;
        for (group, policy) in policy::by_group() {
            let record = bind(laser, &provisioned.run_file, group, &policy).await?;
            provisioned.run_file.groups.push(record);
            provisioned.run_file.write(&provisioned.path)?;
            let handle = laser
                .stream(run_id.stream())
                .topic(RunTopic::Changes.to_string())
                .consumer_group(group);
            sample_test(
                &handle,
                group,
                &policy,
                settings.codec,
                &provisioned.schemas,
            )
            .await?;
            if settings.workers_per_role > 1 {
                for worker in 1..=settings.workers_per_role {
                    let name = format!("{group}-worker-{worker}");
                    let record = bind(laser, &provisioned.run_file, &name, &policy).await?;
                    provisioned.run_file.groups.push(record);
                    provisioned.run_file.write(&provisioned.path)?;
                }
            }
        }
        provisioned.run_file.write(&provisioned.path)?;
        Ok::<(), DemoError>(())
    }
    .await;
    if let Err(error) = outcome {
        let _ = crate::cleanup::cleanup(laser, &provisioned.run_file).await;
        return Err(error);
    }
    act(&format!("Wrote {}.", provisioned.path.display()));
    Ok(provisioned)
}

// A/B groups own separate definitions, so their policy and offsets remain independent.
async fn bind(
    laser: &Laser,
    run_file: &RunFile,
    group: &str,
    policy: &RolePolicy,
) -> Result<GroupRecord, DemoError> {
    let handle = laser
        .stream(run_file.run_id.stream())
        .topic(RunTopic::Changes.to_string())
        .consumer_group(group);
    let configured = handle
        .create()
        .filter(policy.filter(run_file.codec, &run_file.schema_ids))
        .build()
        .await?;
    let binding = configured.filter.ok_or_else(|| {
        DemoError::Incomplete(format!(
            "group {group} was created without its requested policy"
        ))
    })?;
    act(&format!(
        "Configured group {group}, id {}, filter {} revision {}.",
        binding.identity.group_id, binding.filter_id, binding.revision
    ));
    Ok(GroupRecord {
        group: group.to_owned(),
        digest: hex(&binding.digest.0),
        binding: Some(binding),
    })
}

// Each team's filter is tried on events it must select and must reject before any record is published.
async fn sample_test(
    consumer_group: &ConsumerGroup,
    group: &str,
    policy: &RolePolicy,
    codec: Codec,
    schemas: &SchemaSet,
) -> Result<(), DemoError> {
    let filter = policy.filter(codec, &schemas.ids());
    let reads = if filter.codec != laser_sdk::filters::FilterCodec::HeadersOnly {
        "payload and headers"
    } else {
        "headers only"
    };
    let mut verdicts = Vec::new();
    for (label, event, override_severity) in samples(group, policy) {
        let payload = codec.encode(&event, schemas)?;
        let mut headers = filter_headers(&codec.headers(&event, schemas)?)?;
        if let Some(text) = override_severity {
            replace_severity(&mut headers, text);
        }
        let tested = consumer_group.filter().test(payload, headers).await?;
        let expected = if matches!(label, Sample::Selects(_)) {
            Verdict::Selected
        } else {
            Verdict::Rejected
        };
        if tested.explanation.verdict != expected {
            return Err(DemoError::Preflight(format!(
                "The {group} filter gave {:?} where it {}.",
                tested.explanation.verdict,
                label.text()
            )));
        }
        verdicts.push(label.text());
    }
    act(&format!(
        "Filter {group} reads {reads}. It {}.",
        verdicts.join(", ")
    ));
    Ok(())
}

enum Sample {
    Selects(&'static str),
    Rejects(&'static str),
}

impl Sample {
    fn text(&self) -> String {
        match self {
            Sample::Selects(what) => format!("selects {what}"),
            Sample::Rejects(what) => format!("rejects {what}"),
        }
    }
}

fn samples(group: &str, policy: &RolePolicy) -> Vec<(Sample, FleetEvent, Option<&'static str>)> {
    match policy.role {
        Role::FoodSafety if group == SAFETY_CURRENT_GROUP => vec![
            (
                Sample::Selects("a truck turning unsafe"),
                fixtures::enters_unsafe(),
                None,
            ),
            (
                Sample::Selects("a battery update of a truck already unsafe"),
                fixtures::battery_while_unsafe(),
                None,
            ),
        ],
        Role::FoodSafety => vec![
            (
                Sample::Selects("a truck turning unsafe"),
                fixtures::enters_unsafe(),
                None,
            ),
            (
                Sample::Rejects("a battery update of a truck already unsafe"),
                fixtures::battery_while_unsafe(),
                None,
            ),
            (
                Sample::Selects("a frozen truck leaving the fleet"),
                fixtures::retired(Cargo::Frozen),
                None,
            ),
        ],
        Role::Maintenance => vec![
            (
                Sample::Selects("a reefer fault"),
                fixtures::reefer_fault(),
                None,
            ),
            (
                Sample::Rejects("the same fault with severity written as the text \"2\""),
                fixtures::reefer_fault(),
                Some("2"),
            ),
        ],
        Role::Regional => vec![
            (
                Sample::Selects("a north pharma reading"),
                fixtures::reefer_fault(),
                None,
            ),
            (
                Sample::Rejects("a south pharma update"),
                fixtures::routine_update(Region::South, Cargo::Pharma),
                None,
            ),
        ],
    }
}

fn replace_severity(headers: &mut [FilterHeader], text: &str) {
    for header in headers
        .iter_mut()
        .filter(|header| header.key == Header::Severity.name())
    {
        header.value = HeaderScalar::String(text.to_owned());
    }
}
