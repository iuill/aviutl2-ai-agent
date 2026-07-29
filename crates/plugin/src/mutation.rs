use aviutl2_ai_agent_protocol::{MoveObjectDestination, TimelineObject};

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveValidationError {
    TargetNotFound,
    TargetAmbiguous,
    FrameOverflow,
    DestinationOccupied,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn locate_exact(
    objects: &[TimelineObject],
    target: &TimelineObject,
) -> Result<usize, MoveValidationError> {
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
    Ok(*target_index)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn validate_move(
    objects: &[TimelineObject],
    target: &TimelineObject,
    destination: &MoveObjectDestination,
) -> Result<(usize, TimelineObject), MoveValidationError> {
    let target_index = locate_exact(objects, target)?;

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
        index != target_index
            && object.layer == destination.layer
            && destination.start_frame <= object.end_frame
            && object.start_frame <= end_frame
    });
    if overlaps {
        return Err(MoveValidationError::DestinationOccupied);
    }

    Ok((
        target_index,
        TimelineObject {
            layer: destination.layer,
            start_frame: destination.start_frame,
            end_frame,
            name: target.name.clone(),
        },
    ))
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn validate_create(
    objects: &[TimelineObject],
    layer: usize,
    start_frame: usize,
    length: usize,
) -> Result<TimelineObject, MoveValidationError> {
    let end_frame = start_frame
        .checked_add(
            length
                .checked_sub(1)
                .ok_or(MoveValidationError::FrameOverflow)?,
        )
        .ok_or(MoveValidationError::FrameOverflow)?;
    if objects.iter().any(|object| {
        object.layer == layer && start_frame <= object.end_frame && object.start_frame <= end_frame
    }) {
        return Err(MoveValidationError::DestinationOccupied);
    }
    Ok(TimelineObject {
        layer,
        start_frame,
        end_frame,
        name: None,
    })
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn validate_duplicate(
    objects: &[TimelineObject],
    target: &TimelineObject,
    destination: &MoveObjectDestination,
) -> Result<(usize, TimelineObject), MoveValidationError> {
    let target_index = locate_exact(objects, target)?;
    let length = target
        .end_frame
        .checked_sub(target.start_frame)
        .and_then(|difference| difference.checked_add(1))
        .ok_or(MoveValidationError::FrameOverflow)?;
    let duplicate = validate_create(objects, destination.layer, destination.start_frame, length)?;
    Ok((
        target_index,
        TimelineObject {
            name: target.name.clone(),
            ..duplicate
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
            &MoveObjectDestination {
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
                &MoveObjectDestination {
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
                &MoveObjectDestination {
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
                &MoveObjectDestination {
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
                &MoveObjectDestination {
                    layer: 0,
                    start_frame: usize::MAX,
                }
            ),
            Err(MoveValidationError::FrameOverflow)
        );
    }

    #[test]
    fn validates_create_range_and_collision() {
        assert_eq!(
            validate_create(&[], 1, 100, 30).unwrap(),
            TimelineObject {
                layer: 1,
                start_frame: 100,
                end_frame: 129,
                name: None,
            }
        );
        assert_eq!(
            validate_create(&[], 1, 100, 0),
            Err(MoveValidationError::FrameOverflow)
        );
        assert_eq!(
            validate_create(&[object(1, 129, 150)], 1, 100, 30),
            Err(MoveValidationError::DestinationOccupied)
        );
    }

    #[test]
    fn validates_duplicate_target_and_destination() {
        let target = object(0, 10, 39);
        let (index, duplicate) = validate_duplicate(
            std::slice::from_ref(&target),
            &target,
            &MoveObjectDestination {
                layer: 1,
                start_frame: 100,
            },
        )
        .unwrap();
        assert_eq!(index, 0);
        assert_eq!(duplicate, object(1, 100, 129));
        assert_eq!(
            validate_duplicate(
                std::slice::from_ref(&target),
                &target,
                &MoveObjectDestination {
                    layer: 0,
                    start_frame: 20,
                },
            ),
            Err(MoveValidationError::DestinationOccupied)
        );
    }
}
