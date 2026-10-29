use super::{Grid, Placed, buckets, masonry, visible};

const GRID: Grid = Grid {
    width: 1000.0,
    card: 300.0,
    gap: 10.0,
    heading: 30.0,
};

#[test]
fn a_narrow_window_still_has_one_column() {
    assert_eq!(
        Grid {
            width: 10.0,
            ..GRID
        }
        .columns(),
        1
    );
}

#[test]
fn columns_fit_cards_and_gaps() {
    assert_eq!(GRID.columns(), 3);
    assert_eq!(
        Grid {
            width: 919.0,
            ..GRID
        }
        .columns(),
        2
    );
    assert_eq!(
        Grid {
            width: 920.0,
            ..GRID
        }
        .columns(),
        3
    );
}

#[test]
fn no_sections_have_no_height() {
    assert_eq!(masonry(&[], GRID).height, 0.0);
}

#[test]
fn an_empty_section_is_only_its_heading() {
    let layout = masonry(&[vec![]], GRID);
    assert_eq!(layout.headings, [(0, 0.0)]);
    assert_eq!(layout.height, 30.0);
}

#[test]
fn one_card_sits_under_its_heading() {
    let layout = masonry(&[vec![100.0]], GRID);
    assert_eq!(
        layout.cards,
        [Placed {
            index: 0,
            x: 0.0,
            y: 30.0,
            height: 100.0
        }]
    );
    assert_eq!(layout.height, 140.0);
}

#[test]
fn each_card_goes_to_the_shortest_column() {
    let layout = masonry(&[vec![300.0, 100.0, 100.0, 50.0]], GRID);
    let spots: Vec<(f32, f32)> = layout.cards.iter().map(|p| (p.x, p.y)).collect();
    assert_eq!(
        spots,
        [(0.0, 30.0), (310.0, 30.0), (620.0, 30.0), (310.0, 140.0)]
    );
    assert_eq!(layout.height, 340.0);
}

#[test]
fn sections_stack_and_number_cards_across_them() {
    let layout = masonry(&[vec![100.0], vec![50.0]], GRID);
    assert_eq!(layout.headings, [(0, 0.0), (1, 140.0)]);
    assert_eq!(
        layout.cards[1],
        Placed {
            index: 1,
            x: 0.0,
            y: 170.0,
            height: 50.0
        }
    );
}

#[test]
fn a_card_is_visible_when_it_overlaps_the_viewport() {
    let card = Placed {
        index: 0,
        x: 0.0,
        y: 100.0,
        height: 50.0,
    };
    assert!(visible(&card, 0.0, 101.0));
    assert!(!visible(&card, 0.0, 100.0));
    assert!(visible(&card, 149.0, 10.0));
    assert!(!visible(&card, 150.0, 10.0));
}

#[test]
fn too_little_history_has_no_bars() {
    assert_eq!(buckets(&[1.0], 10), Vec::<f32>::new());
    assert_eq!(buckets(&[1.0, 2.0], 0), Vec::<f32>::new());
}

#[test]
fn two_values_make_two_bars() {
    assert_eq!(buckets(&[1.0, 2.0], 2), [0.0, 1.0]);
}

#[test]
fn bars_scale_between_the_lowest_and_highest_value() {
    assert_eq!(buckets(&[10.0, 20.0, 15.0], 3), [0.0, 1.0, 0.5]);
}

#[test]
fn a_flat_series_has_flat_bars() {
    assert_eq!(buckets(&[5.0, 5.0, 5.0], 3), [0.0, 0.0, 0.0]);
}

#[test]
fn many_values_bucket_to_their_peak() {
    assert_eq!(
        buckets(&[0.0, 4.0, 1.0, 2.0, 3.0, 0.0], 3),
        [1.0, 0.5, 0.75]
    );
}
