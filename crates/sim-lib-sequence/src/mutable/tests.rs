#[cfg(test)]
mod tests {
    use super::{CompactionResult, OrderedSet, OrderedTable, SparseSequence, SparseSequenceError};

    fn exact(left: &&str, right: &&str) -> bool {
        left == right
    }

    #[test]
    fn delete_and_reinsert_moves_key_to_end() {
        let table = OrderedTable::new(exact);
        table.insert("first", 1);
        table.insert("second", 2);
        assert_eq!(table.insert("first", 3), Some(1));
        assert_eq!(table.remove(&"first"), Some(3));
        table.insert("first", 4);

        assert_eq!(
            table.iter().collect::<Vec<_>>(),
            vec![("second", 2), ("first", 4)]
        );
    }

    #[test]
    fn iterator_observes_delete_and_append_without_losing_position() {
        let table = OrderedTable::new(exact);
        table.insert("first", 1);
        table.insert("deleted", 2);
        table.insert("third", 3);
        let mut iterator = table.iter();

        assert_eq!(iterator.next(), Some(("first", 1)));
        table.remove(&"deleted");
        table.insert("appended", 4);
        assert_eq!(
            iterator.collect::<Vec<_>>(),
            vec![("third", 3), ("appended", 4)]
        );
    }

    #[test]
    fn compaction_is_bounded_and_blocked_by_active_iterator() {
        let table = OrderedTable::new(exact);
        table.insert("first", 1);
        table.insert("deleted", 2);
        table.insert("third", 3);
        table.remove(&"deleted");
        let iterator = table.iter();

        assert_eq!(table.compact(usize::MAX), CompactionResult::ActiveIterator);
        assert_eq!(table.slot_len(), 3);
        drop(iterator);
        assert_eq!(
            table.compact(2),
            CompactionResult::BudgetExceeded { required: 3 }
        );
        assert_eq!(table.slot_len(), 3);
        assert_eq!(table.compact(3), CompactionResult::Compacted(1));
        assert_eq!(table.slot_len(), 2);
    }

    #[test]
    fn set_uses_the_supplied_equivalence_policy() {
        let set = OrderedSet::new(|left: &String, right: &String| left.eq_ignore_ascii_case(right));
        assert!(set.insert("Alpha".to_owned()));
        assert!(!set.insert("alpha".to_owned()));
        assert!(set.contains(&"ALPHA".to_owned()));
        assert_eq!(set.iter().collect::<Vec<_>>(), vec!["Alpha".to_owned()]);
    }

    #[test]
    fn distant_write_allocates_only_occupied_storage() {
        let mut sequence = SparseSequence::new(1_000_001);
        sequence.set(1_000_000, "far").unwrap();

        assert_eq!(sequence.len(), 1_000_001);
        assert_eq!(sequence.occupied_len(), 1);
        assert_eq!(sequence.chunks.len(), 1);
        assert_eq!(sequence.get(999_999), None);
        assert_eq!(sequence.get(1_000_000), Some(&"far"));
    }

    #[test]
    fn generated_operations_preserve_sparse_model() {
        let mut sequence = SparseSequence::new(257);
        let mut model = Vec::<Option<u32>>::new();
        let mut state = 0x5eed_u64;

        for _ in 0..2_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let index = (state as usize) % 257;
            match (state >> 32) % 3 {
                0 => {
                    let value = state as u32;
                    sequence.set(index, value).unwrap();
                    model.resize(model.len().max(index + 1), None);
                    model[index] = Some(value);
                }
                1 => {
                    assert_eq!(
                        sequence.remove(index),
                        model.get_mut(index).and_then(Option::take)
                    );
                }
                _ => {
                    let new_len = index;
                    sequence.set_len(new_len).unwrap();
                    model.resize(new_len, None);
                }
            }

            assert_eq!(sequence.len(), model.len());
            assert_eq!(sequence.occupied_len(), model.iter().flatten().count());
            assert_eq!(
                sequence
                    .occupied_in(..)
                    .map(|(index, value)| (index, *value))
                    .collect::<Vec<_>>(),
                model
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| value.map(|value| (index, value)))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn growth_limits_fail_without_mutation() {
        let mut sequence = SparseSequence::new(4);
        sequence.set(3, 7).unwrap();
        let revision = sequence.revision();

        assert_eq!(
            sequence.set(4, 8),
            Err(SparseSequenceError::LengthLimit {
                requested: 5,
                limit: 4
            })
        );
        assert_eq!(
            sequence.set_len(5),
            Err(SparseSequenceError::LengthLimit {
                requested: 5,
                limit: 4
            })
        );
        assert_eq!(sequence.revision(), revision);
        assert_eq!(sequence.get(3), Some(&7));
    }

    #[test]
    fn truncation_drops_values_and_growth_restores_holes() {
        let mut sequence = SparseSequence::new(200);
        sequence.set(2, 'a').unwrap();
        sequence.set(130, 'b').unwrap();
        sequence.set_len(64).unwrap();
        sequence.set_len(131).unwrap();

        assert_eq!(sequence.get(2), Some(&'a'));
        assert_eq!(sequence.get(130), None);
        assert_eq!(sequence.occupied_len(), 1);
        assert_eq!(sequence.remove(2), Some('a'));
        assert_eq!(sequence.len(), 131);
        assert_eq!(sequence.occupied_len(), 0);
    }
}
