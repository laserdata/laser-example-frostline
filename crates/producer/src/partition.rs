use frostline_shared::domain::TruckId;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a over the truck id, so a truck always lands on the same partition whatever the process.
pub fn partition_for(truck: &TruckId, partitions: u32) -> u32 {
    let hash = truck.as_str().bytes().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    });
    u32::try_from(hash % u64::from(partitions.max(1))).expect("a partition index fits in u32")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_known_truck_when_partitioned_then_should_match_the_fixed_vector() {
        assert_eq!(partition_for(&TruckId::numbered(42), 4), 2);
        assert_eq!(partition_for(&TruckId::numbered(42), 1), 0);
    }

    #[test]
    fn given_the_default_fleet_when_partitioned_then_should_spread_evenly() {
        let mut counts = [0u32; 4];
        for number in 1..=800 {
            counts[partition_for(&TruckId::numbered(number), 4) as usize] += 1;
        }
        for count in counts {
            assert!((180..=220).contains(&count), "{counts:?}");
        }
    }
}
