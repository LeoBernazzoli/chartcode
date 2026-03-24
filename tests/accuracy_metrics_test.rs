use autoclaw::accuracy::compute_accuracy_metrics;
use std::collections::BTreeSet;

#[test]
fn accuracy_metrics_compute_tp_fp_fn_precision_recall_and_f1() {
    let predicted = BTreeSet::from([
        "a.py".to_string(),
        "b.py".to_string(),
        "c.py".to_string(),
    ]);
    let expected = BTreeSet::from([
        "b.py".to_string(),
        "c.py".to_string(),
        "d.py".to_string(),
    ]);

    let metrics = compute_accuracy_metrics(&predicted, &expected);

    assert_eq!(metrics.true_positives, 2);
    assert_eq!(metrics.false_positives, 1);
    assert_eq!(metrics.false_negatives, 1);
    assert_eq!(metrics.true_positive_files, BTreeSet::from([
        "b.py".to_string(),
        "c.py".to_string(),
    ]));
    assert_eq!(metrics.false_positive_files, BTreeSet::from([
        "a.py".to_string(),
    ]));
    assert_eq!(metrics.false_negative_files, BTreeSet::from([
        "d.py".to_string(),
    ]));
    assert!((metrics.precision - (2.0 / 3.0)).abs() < 0.0001);
    assert!((metrics.recall - (2.0 / 3.0)).abs() < 0.0001);
    assert!((metrics.f1 - (2.0 / 3.0)).abs() < 0.0001);
}
