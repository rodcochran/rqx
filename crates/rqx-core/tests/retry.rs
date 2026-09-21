// TODO: determine if these tests are even worth it (leaning toward no)...
//
// #[cfg(test)]
// mod tests {
//     use super::*;

//     const KINDS: [FailureKind; 3] = [FailureKind::Connect, FailureKind::Read, FailureKind::Status];

//     fn config(total: i32, connect: i32, read: i32, status: i32) -> PyRetry {
//         PyRetry {
//             inner: Retry::new(
//                 Some(total),
//                 Some(connect),
//                 Some(read),
//                 Some(status),
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//             ),
//         }
//     }

//     fn cap(r: &PyRetry, kind: FailureKind) -> i32 {
//         match kind {
//             FailureKind::Connect => r.inner.connect,
//             FailureKind::Read => r.inner.read,
//             FailureKind::Status => r.inner.status,
//         }
//     }

//     fn count(used: &RetryCounts, kind: FailureKind) -> i32 {
//         match kind {
//             FailureKind::Connect => used.connect,
//             FailureKind::Read => used.read,
//             FailureKind::Status => used.status,
//         }
//     }

//     /// Every config with caps in 0..=3 and every count vector in the same
//     /// range: the space is small enough to enumerate outright, which beats
//     /// sampling it (https://github.com/rodcochran/rqx/issues/44).
//     fn every_config_and_state(mut check: impl FnMut(&PyRetry, &RetryCounts)) {
//         for total in 0..=3 {
//             for connect in 0..=3 {
//                 for read in 0..=3 {
//                     for status in 0..=3 {
//                         let r = config(
//                             total, connect, read, status,
//                         );
//                         for c in 0..=3 {
//                             for rd in 0..=3 {
//                                 for st in 0..=3 {
//                                     let used = RetryCounts {
//                                         total: c + rd + st,
//                                         connect: c,
//                                         read: rd,
//                                         status: st,
//                                     };
//                                     check(
//                                         &r, &used,
//                                     );
//                                 }
//                             }
//                         }
//                     }
//                 }
//             }
//         }
//     }

//     #[test]
//     fn never_allows_past_the_total_or_the_kind_cap() {
//         every_config_and_state(
//             |r, used| {
//                 for kind in KINDS {
//                     let allowed = r.allows_another(
//                         kind, used,
//                     );
//                     if used.total >= r.inner.total
//                         || count(
//                             used, kind,
//                         ) >= cap(
//                             r, kind,
//                         )
//                     {
//                         assert!(
//                             !allowed,
//                             "allowed past a cap: {:?}",
//                             kind
//                         );
//                     } else {
//                         assert!(
//                             allowed,
//                             "refused inside every cap: {:?}",
//                             kind
//                         );
//                     }
//                 }
//             },
//         );
//     }

//     #[test]
//     fn once_refused_a_kind_stays_refused_as_counts_grow() {
//         every_config_and_state(
//             |r, used| {
//                 for kind in KINDS {
//                     if r.allows_another(
//                         kind, used,
//                     ) {
//                         continue;
//                     }
//                     for more in KINDS {
//                         let mut grown = RetryCounts {
//                             total: used.total,
//                             connect: used.connect,
//                             read: used.read,
//                             status: used.status,
//                         };
//                         grown.record(more);
//                         assert!(
//                             !r.allows_another(
//                                 kind, &grown
//                             )
//                         );
//                     }
//                 }
//             },
//         );
//     }

//     #[test]
//     fn a_single_kind_sequence_stops_at_the_smaller_cap() {
//         for total in 0..=3 {
//             for cap_value in 0..=3 {
//                 for kind in KINDS {
//                     let r = match kind {
//                         FailureKind::Connect => config(
//                             total, cap_value, 3, 3,
//                         ),
//                         FailureKind::Read => config(
//                             total, 3, cap_value, 3,
//                         ),
//                         FailureKind::Status => config(
//                             total, 3, 3, cap_value,
//                         ),
//                     };
//                     let mut used = RetryCounts::default();
//                     while r.allows_another(
//                         kind, &used,
//                     ) {
//                         used.record(kind);
//                     }
//                     assert_eq!(
//                         count(
//                             &used, kind
//                         ),
//                         total.min(cap_value)
//                     );
//                     assert_eq!(
//                         used.total,
//                         total.min(cap_value)
//                     );
//                 }
//             }
//         }
//     }
// }
