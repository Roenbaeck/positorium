use positorium::construct::{Appearance, AppearanceSet, Posit, Role};
use positorium::datatype::Time;
use positorium::literal::{LiteralFamily, LiteralValue};
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

fn hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn equal<T: Eq>(left: &T, right: &T) -> bool {
    left.eq(right)
}

fn assert_eq_hash_ord_laws<T: Eq + Ord + Hash + Debug>(values: &[T]) {
    for left in values {
        assert!(equal(left, left), "equality must be reflexive for {left:?}");
        assert_eq!(left.cmp(left), Ordering::Equal);
        for right in values {
            assert_eq!(
                equal(left, right),
                equal(right, left),
                "equality must be symmetric"
            );
            assert_eq!(
                left.cmp(right),
                right.cmp(left).reverse(),
                "ordering must be antisymmetric for {left:?} and {right:?}"
            );
            assert_eq!(
                equal(left, right),
                left.cmp(right) == Ordering::Equal,
                "Eq and Ord disagree for {left:?} and {right:?}"
            );
            if equal(left, right) {
                assert_eq!(hash(left), hash(right), "equal values must hash equally");
            }
            for third in values {
                if left <= right && right <= third {
                    assert!(
                        left <= third,
                        "ordering is not transitive for {left:?}, {right:?}, {third:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn core_constructs_obey_eq_hash_and_total_order_laws() {
    let roles = [
        Role::new(10, "name".to_string(), false),
        Role::new(10, "different metadata".to_string(), true),
        Role::new(11, "name".to_string(), false),
    ];
    assert_eq_hash_ord_laws(&roles);

    let name = Arc::new(Role::new(10, "name".to_string(), false));
    let age = Arc::new(Role::new(11, "age".to_string(), false));
    let appearances = [
        Appearance::new(20, Arc::clone(&name)),
        Appearance::new(20, Arc::clone(&name)),
        Appearance::new(21, Arc::clone(&name)),
        Appearance::new(20, Arc::clone(&age)),
    ];
    assert_eq_hash_ord_laws(&appearances);

    let name_20 = Arc::new(Appearance::new(20, Arc::clone(&name)));
    let name_21 = Arc::new(Appearance::new(21, Arc::clone(&name)));
    let age_20 = Arc::new(Appearance::new(20, Arc::clone(&age)));
    let sets = [
        AppearanceSet::new(vec![Arc::clone(&name_20)]).unwrap(),
        AppearanceSet::new(vec![Arc::clone(&name_20)]).unwrap(),
        AppearanceSet::new(vec![Arc::clone(&name_21)]).unwrap(),
        AppearanceSet::new(vec![Arc::clone(&age_20), Arc::clone(&name_20)]).unwrap(),
        AppearanceSet::new(vec![Arc::clone(&name_20), Arc::clone(&age_20)]).unwrap(),
    ];
    assert_eq_hash_ord_laws(&sets);

    let literals = [
        LiteralValue::new("10", LiteralFamily::Integer).unwrap(),
        LiteralValue::new("10", LiteralFamily::Integer).unwrap(),
        LiteralValue::new("10.00", LiteralFamily::Decimal).unwrap(),
        LiteralValue::new(r#""10""#, LiteralFamily::String).unwrap(),
        LiteralValue::new("075%", LiteralFamily::Certainty).unwrap(),
    ];
    assert_eq_hash_ord_laws(&literals);

    let times = [
        Time::new_beginning_of_time(),
        Time::new_year_from("2024").unwrap(),
        Time::new_year_month_from("2024-05").unwrap(),
        Time::new_date_from("2024-05-06").unwrap(),
        Time::new_date_from("2024-05-06").unwrap(),
        Time::new_datetime_from("2024-05-06T07:08:09.123456789").unwrap(),
        Time::new_end_of_time(),
    ];
    assert_eq_hash_ord_laws(&times);

    let set_a = Arc::new(AppearanceSet::new(vec![Arc::clone(&name_20)]).unwrap());
    let set_b = Arc::new(AppearanceSet::new(vec![Arc::clone(&name_21)]).unwrap());
    let posits = [
        Posit::new(
            100,
            Arc::clone(&set_a),
            literals[0].clone(),
            times[1].clone(),
        ),
        Posit::new(
            101,
            Arc::clone(&set_a),
            literals[0].clone(),
            times[1].clone(),
        ),
        Posit::new(
            102,
            Arc::clone(&set_a),
            literals[2].clone(),
            times[1].clone(),
        ),
        Posit::new(
            103,
            Arc::clone(&set_b),
            literals[0].clone(),
            times[3].clone(),
        ),
    ];
    assert_eq_hash_ord_laws(&posits);
}
