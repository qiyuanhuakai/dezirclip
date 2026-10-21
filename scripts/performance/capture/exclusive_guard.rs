pub(crate) fn measure_guard_excluding_publish<R>(
    mut now: impl FnMut() -> std::time::Duration,
    operation: impl FnOnce(&mut dyn FnMut()) -> R,
    publish: &mut dyn FnMut(),
) -> (R, std::time::Duration) {
    let start = now();
    let mut published = std::time::Duration::ZERO;
    let result = operation(&mut || {
        let start = now();
        publish();
        published += now().checked_sub(start).expect("monotonic publish clock");
    });
    let total = now().checked_sub(start).expect("monotonic guard clock");
    let exclusive = total.checked_sub(published)
        .expect("publish duration must be contained in its capture guard");
    (result, exclusive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn guard_excludes_commit_and_does_not_borrow_accounting_across_publish() {
        let clock = Cell::new(Duration::ZERO);
        let times = RefCell::new(BTreeMap::<String, f64>::new());
        let (result, guard) = measure_guard_excluding_publish(
            || clock.get(),
            |publish| {
                clock.set(clock.get() + Duration::from_millis(3));
                publish();
                clock.set(clock.get() + Duration::from_millis(7));
                "committed"
            },
            &mut || {
                // The production callback re-enters TimedStore::commit.
                *times.borrow_mut().entry("commit_ms".into()).or_default() += 90.0;
                clock.set(clock.get() + Duration::from_millis(90));
            },
        );
        times.borrow_mut().insert("privacy_guard_ms".into(), guard.as_secs_f64() * 1000.0);
        assert_eq!(result, "committed");
        assert_eq!(guard, Duration::from_millis(10));
        assert_eq!(times.into_inner().values().sum::<f64>(), 100.0);
        assert_eq!(clock.get(), Duration::from_millis(100));
    }

    #[test]
    fn guard_excludes_every_publish_but_keeps_authorization_between_calls() {
        let clock = Cell::new(Duration::ZERO);
        let (_, guard) = measure_guard_excluding_publish(
            || clock.get(),
            |publish| {
                for _ in 0..2 {
                    clock.set(clock.get() + Duration::from_millis(5));
                    publish();
                }
            },
            &mut || clock.set(clock.get() + Duration::from_millis(40)),
        );
        assert_eq!(guard, Duration::from_millis(10));
        assert_eq!(clock.get(), Duration::from_millis(90));
    }

    #[test]
    fn rejected_guard_records_authorization_without_calling_publish() {
        let clock = Cell::new(Duration::ZERO);
        let (result, guard) = measure_guard_excluding_publish(
            || clock.get(),
            |_| {
                clock.set(Duration::from_millis(12));
                Err::<(), _>("authorization rejected")
            },
            &mut || panic!("rejected authorization must not publish"),
        );
        assert_eq!(result, Err("authorization rejected"));
        assert_eq!(guard, Duration::from_millis(12));
    }

    #[test]
    #[should_panic(expected = "publish duration must be contained")]
    fn impossible_clock_partition_is_rejected_instead_of_clamped() {
        let mut readings = [0, 0, 20, 10].into_iter();
        measure_guard_excluding_publish(
            || Duration::from_millis(readings.next().unwrap()),
            |publish| publish(),
            &mut || {},
        );
    }

}
