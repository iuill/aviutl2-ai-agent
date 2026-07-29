use aviutl2_ai_agent_protocol::TimelineObject;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MoveDestination {
    pub(crate) layer: usize,
    pub(crate) start_frame: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveValidationError {
    TargetNotFound,
    TargetAmbiguous,
    FrameOverflow,
    DestinationOccupied,
}

pub(crate) fn validate_move(
    objects: &[TimelineObject],
    target: &TimelineObject,
    destination: MoveDestination,
) -> Result<(usize, TimelineObject), MoveValidationError> {
    let matches = objects
        .iter()
        .enumerate()
        .filter(|(_, object)| *object == target)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [target_index] = matches.as_slice() else {
        return Err(if matches.is_empty() {
            MoveValidationError::TargetNotFound
        } else {
            MoveValidationError::TargetAmbiguous
        });
    };

    let length = target
        .end_frame
        .checked_sub(target.start_frame)
        .and_then(|difference| difference.checked_add(1))
        .ok_or(MoveValidationError::FrameOverflow)?;
    let end_frame = destination
        .start_frame
        .checked_add(length - 1)
        .ok_or(MoveValidationError::FrameOverflow)?;

    let overlaps = objects.iter().enumerate().any(|(index, object)| {
        index != *target_index
            && object.layer == destination.layer
            && destination.start_frame <= object.end_frame
            && object.start_frame <= end_frame
    });
    if overlaps {
        return Err(MoveValidationError::DestinationOccupied);
    }

    Ok((
        *target_index,
        TimelineObject {
            layer: destination.layer,
            start_frame: destination.start_frame,
            end_frame,
            name: target.name.clone(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(layer: usize, start_frame: usize, end_frame: usize) -> TimelineObject {
        TimelineObject {
            layer,
            start_frame,
            end_frame,
            name: Some("Text".to_owned()),
        }
    }

    #[test]
    fn resolves_exact_snapshot_and_preserves_length() {
        let target = object(0, 10, 39);
        let objects = vec![target.clone()];
        let (index, moved) = validate_move(
            &objects,
            &target,
            MoveDestination {
                layer: 2,
                start_frame: 100,
            },
        )
        .unwrap();
        assert_eq!(index, 0);
        assert_eq!(moved, object(2, 100, 129));
    }

    #[test]
    fn rejects_missing_or_ambiguous_snapshot() {
        let target = object(0, 10, 39);
        assert_eq!(
            validate_move(
                &[],
                &target,
                MoveDestination {
                    layer: 1,
                    start_frame: 0,
                }
            ),
            Err(MoveValidationError::TargetNotFound)
        );
        assert_eq!(
            validate_move(
                &[target.clone(), target.clone()],
                &target,
                MoveDestination {
                    layer: 1,
                    start_frame: 0,
                }
            ),
            Err(MoveValidationError::TargetAmbiguous)
        );
    }

    #[test]
    fn rejects_inclusive_destination_overlap() {
        let target = object(0, 10, 19);
        let occupied = object(1, 20, 29);
        assert_eq!(
            validate_move(
                &[target.clone(), occupied],
                &target,
                MoveDestination {
                    layer: 1,
                    start_frame: 11,
                }
            ),
            Err(MoveValidationError::DestinationOccupied)
        );
    }

    #[test]
    fn rejects_frame_overflow() {
        let target = object(0, 0, 9);
        assert_eq!(
            validate_move(
                &[target.clone()],
                &target,
                MoveDestination {
                    layer: 0,
                    start_frame: usize::MAX,
                }
            ),
            Err(MoveValidationError::FrameOverflow)
        );
    }
}
